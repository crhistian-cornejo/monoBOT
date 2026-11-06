//! A group's Activity panel (`groupActivity.ts`): each member, what it is
//! doing in the group, the subagents of its latest turn and what it spent,
//! and what the members asked each other lately.
use super::{
    block::Block,
    brief::{group_members, member_role, Member},
    group::GroupMono,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// What an agent is doing right now, as the Monos roster shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MonoStatus {
    Working,
    NeedsYou,
    #[default]
    Idle,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MonoState {
    pub status: MonoStatus,
    /// Short present-tense note while working or waiting.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
}

impl MonoState {
    pub fn new(status: MonoStatus) -> Self {
        Self {
            status,
            activity: None,
        }
    }
}

/// A conversation as the panel reads it: the group's chat or a seat.
#[derive(Clone, Debug, Default)]
pub struct ActivitySession<'a> {
    pub id: &'a str,
    pub cwd: Option<&'a str>,
    pub harness: Option<&'a str>,
    /// The display name of the model it answers with
    /// (`resolveModel(harness, model).name`).
    pub model: &'a str,
    pub blocks: &'a [Block],
    /// What it is doing (`monoState(session)`), which the caller reads from
    /// its busy flag, pending question, approvals and usage limit.
    pub state: MonoState,
}

/// What a member has spent in its group: its seat's turns, tokens and model.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeatUsage {
    pub turns: usize,
    pub tokens: u64,
    pub model: String,
}

/// One row of a group's activity: a member, what it is doing, and the
/// subagents it launched in its latest turn. Its mascot and color are the
/// roster's, by `mono_id`.
#[derive(Clone, Debug, PartialEq)]
pub struct MemberActivity<'a> {
    pub mono_id: String,
    pub name: String,
    pub role: String,
    /// The Principal, whose work in the group is the group chat's.
    pub lead: bool,
    pub state: MonoState,
    /// Its latest turn's subagent runs, in the order they started.
    pub runs: Vec<&'a Block>,
    /// Where its chat runs, for the paths its subagents show.
    pub cwd: Option<String>,
    pub usage: Option<SeatUsage>,
    /// When its current turn began, while it works or waits on the user.
    pub started_at: Option<i64>,
}

/// The subagent runs of the session's latest turn.
pub fn latest_runs(blocks: &[Block]) -> Vec<&Block> {
    let start = blocks
        .iter()
        .rposition(Block::is_user_turn)
        .map_or(0, |index| index + 1);
    blocks[start..]
        .iter()
        .filter(|block| block.agent_run.is_some())
        .collect()
}

/// When the session's latest turn began.
pub fn turn_started_at(blocks: &[Block]) -> Option<i64> {
    blocks
        .iter()
        .rev()
        .find(|block| block.is_user_turn())?
        .started_at
}

/// A seat's turns and tokens in its group, and the model it answers with.
pub fn seat_usage(blocks: &[Block], model: &str) -> SeatUsage {
    let mut usage = SeatUsage {
        model: model.into(),
        ..SeatUsage::default()
    };
    for block in blocks.iter().filter(|block| block.is_user_turn()) {
        usage.turns += 1;
        if let Some(metrics) = &block.turn_metrics {
            usage.tokens += metrics.input_tokens.unwrap_or(0) + metrics.output_tokens.unwrap_or(0);
        }
    }
    usage
}

/// The group's members as its Activity panel lists them, one under the
/// other: the Principal first, read from the group chat it leads, then each
/// member from its seat, its own conversation in the group. A member that
/// has not spoken in the group yet is idle here, whatever its chat is doing.
/// `states` are the roster's states by Mono id, for a lead whose chat is
/// not loaded.
pub fn group_activity<'a>(
    group: &GroupMono,
    roster: &[Member],
    sessions: &'a [ActivitySession<'a>],
    states: &HashMap<String, MonoState>,
) -> Vec<MemberActivity<'a>> {
    let session_of =
        |id: Option<&str>| id.and_then(|id| sessions.iter().find(|session| session.id == id));
    let rows: Vec<MemberActivity<'a>> = group_members(&group.group, roster)
        .into_iter()
        .map(|member| {
            let lead = member.is_principal();
            let session = session_of(if lead {
                group.session_id.as_deref()
            } else {
                group.group.seats.get(&member.id).map(String::as_str)
            });
            let state = match session {
                Some(session) => session.state.clone(),
                None if lead => states.get(&group.id).cloned().unwrap_or_default(),
                None => MonoState::default(),
            };
            let started_at = session
                .filter(|_| state.status != MonoStatus::Idle)
                .and_then(|session| turn_started_at(session.blocks));
            MemberActivity {
                mono_id: member.id.clone(),
                name: member.name.clone(),
                role: if lead {
                    "Leads the group".into()
                } else {
                    member_role(member)
                },
                lead,
                state,
                runs: session
                    .map(|session| latest_runs(session.blocks))
                    .unwrap_or_default(),
                cwd: session
                    .and_then(|session| session.cwd)
                    .filter(|cwd| !cwd.is_empty())
                    .map(String::from),
                usage: session
                    .filter(|session| session.harness.is_some_and(|harness| !harness.is_empty()))
                    .map(|session| seat_usage(session.blocks, session.model)),
                started_at,
            }
        })
        .collect();
    let (leads, others): (Vec<_>, Vec<_>) = rows.into_iter().partition(|row| row.lead);
    leads.into_iter().chain(others).collect()
}

/// One agent asking another (`monocode.delegations.v1`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Delegation {
    pub id: String,
    /// The Mono that asked.
    pub from: String,
    /// The Mono it asked.
    pub to: String,
    pub message: String,
    pub at: i64,
    /// What it answered, when the asker waited for it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

/// How far back the panel shows members asking each other.
const EXCHANGE_WINDOW_MS: i64 = 24 * 60 * 60 * 1000;
const EXCHANGES_MAX: usize = 8;

/// What the members asked each other lately, newest first (`delegations`
/// come newest first).
pub fn group_exchanges<'a>(
    group: &GroupMono,
    delegations: &'a [Delegation],
    now: i64,
) -> Vec<&'a Delegation> {
    let members: HashSet<&str> = group.group.members.iter().map(String::as_str).collect();
    delegations
        .iter()
        .filter(|entry| {
            members.contains(entry.from.as_str())
                && members.contains(entry.to.as_str())
                && now - entry.at < EXCHANGE_WINDOW_MS
        })
        .take(EXCHANGES_MAX)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::{block::TurnMetrics, brief::MemberKind, group::Group};
    use serde_json::json;
    use std::collections::BTreeMap;

    fn run(id: &str) -> Block {
        Block {
            id: id.into(),
            role: "assistant".into(),
            agent_run: Some(json!({ "name": id, "steps": [] })),
            ..Block::default()
        }
    }
    fn user(id: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            role: "user".into(),
            text: text.into(),
            ..Block::default()
        }
    }
    fn bot(id: &str, name: &str, role: &str) -> Member {
        Member {
            id: id.into(),
            name: name.into(),
            kind: MemberKind::Bot { role: role.into() },
        }
    }
    fn roster() -> Vec<Member> {
        vec![
            bot("ivy", "IVY", "principal"),
            bot("research", "Research", "research"),
            bot("docs", "Documents", "documents"),
        ]
    }
    fn nexa() -> GroupMono {
        GroupMono {
            id: "nexa".into(),
            session_id: Some("nexa-chat".into()),
            name: "Nexa".into(),
            group: Group {
                members: vec!["research".into(), "ivy".into(), "docs".into()],
                seats: BTreeMap::from([("research".into(), "research-seat".into())]),
                ..Group::default()
            },
        }
    }
    fn metrics(input: u64, output: u64) -> Option<TurnMetrics> {
        Some(TurnMetrics {
            input_tokens: Some(input),
            output_tokens: Some(output),
        })
    }
    const WORKING: MonoState = MonoState {
        status: MonoStatus::Working,
        activity: None,
    };

    #[test]
    fn groups_keep_only_the_latest_turns_subagents() {
        let blocks = vec![
            user("u1", "first"),
            run("old"),
            user("u2", "second"),
            Block {
                internal: true,
                ..user("i", "keep going")
            },
            run("a"),
            Block {
                role: "assistant".into(),
                ..user("t", "Working")
            },
            run("b"),
        ];
        let ids: Vec<&str> = latest_runs(&blocks)
            .iter()
            .map(|block| block.id.as_str())
            .collect();
        assert_eq!(ids, vec!["a", "b"]);
    }

    #[test]
    fn groups_list_the_principal_first_from_the_group_chat_then_each_members_seat() {
        let lead_blocks = vec![user("u", "go")];
        let seat_blocks = vec![
            Block {
                turn_metrics: metrics(1200, 300),
                ..user("u", "find")
            },
            run("papers"),
            run("web"),
        ];
        let own_blocks = vec![user("u", "mine")];
        let sessions = vec![
            ActivitySession {
                id: "nexa-chat",
                cwd: Some("/home"),
                blocks: &lead_blocks,
                state: WORKING,
                ..ActivitySession::default()
            },
            ActivitySession {
                id: "research-seat",
                cwd: Some("/home"),
                harness: Some("claude"),
                model: "Sonnet",
                blocks: &seat_blocks,
                state: WORKING,
            },
            // Its own chat is busy with something else: not the group's work.
            ActivitySession {
                id: "docs-chat",
                cwd: Some("/home"),
                blocks: &own_blocks,
                state: WORKING,
                ..ActivitySession::default()
            },
        ];
        let states = HashMap::from([(
            "docs".to_string(),
            MonoState {
                status: MonoStatus::NeedsYou,
                activity: Some("Approve Write".into()),
            },
        )]);
        let roster = roster();
        let rows = group_activity(&nexa(), &roster, &sessions, &states);
        let summary: Vec<(&str, bool, MonoStatus)> = rows
            .iter()
            .map(|row| (row.name.as_str(), row.lead, row.state.status))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("IVY", true, MonoStatus::Working),
                ("Research", false, MonoStatus::Working),
                ("Documents", false, MonoStatus::Idle),
            ]
        );
        let runs: Vec<&str> = rows[1].runs.iter().map(|block| block.id.as_str()).collect();
        assert_eq!(runs, vec!["papers", "web"]);
        assert_eq!(rows[1].cwd.as_deref(), Some("/home"));
        let usage = rows[1].usage.as_ref().unwrap();
        assert_eq!((usage.turns, usage.tokens), (1, 1500));
        assert!(rows[2].runs.is_empty());
        assert_eq!(rows[2].usage, None);
        assert_eq!(rows[0].role, "Leads the group");
        assert_eq!(rows[1].role, "Research bot");
    }

    #[test]
    fn groups_time_a_working_members_turn_and_give_the_lead_its_usage_too() {
        let lead_blocks = vec![Block {
            started_at: Some(5_000),
            turn_metrics: metrics(100, 20),
            ..user("u", "go")
        }];
        let seat_blocks = vec![
            Block {
                started_at: Some(1_000),
                ..user("u1", "first")
            },
            Block {
                started_at: Some(9_000),
                ..user("u2", "find")
            },
            Block {
                started_at: Some(12_000),
                internal: true,
                ..user("i", "keep going")
            },
        ];
        let sessions = vec![
            ActivitySession {
                id: "nexa-chat",
                cwd: Some("/home"),
                harness: Some("claude"),
                model: "Sonnet",
                blocks: &lead_blocks,
                state: MonoState::default(),
            },
            ActivitySession {
                id: "research-seat",
                cwd: Some("/home"),
                harness: Some("claude"),
                model: "Sonnet",
                blocks: &seat_blocks,
                state: WORKING,
            },
        ];
        let roster = roster();
        let rows = group_activity(&nexa(), &roster, &sessions, &HashMap::new());
        let (lead, research) = (&rows[0], &rows[1]);
        assert_eq!(lead.state.status, MonoStatus::Idle);
        assert_eq!(lead.started_at, None);
        let usage = lead.usage.as_ref().unwrap();
        assert_eq!(
            (usage.turns, usage.tokens, usage.model.as_str()),
            (1, 120, "Sonnet")
        );
        assert_eq!(research.state.status, MonoStatus::Working);
        assert_eq!(research.started_at, Some(9_000));
    }

    #[test]
    fn groups_read_the_leads_state_from_the_roster_while_its_chat_is_not_loaded() {
        let states = HashMap::from([("nexa".to_string(), WORKING)]);
        let roster = roster();
        let rows = group_activity(&nexa(), &roster, &[], &states);
        assert_eq!(rows[0].state.status, MonoStatus::Working);
        assert_eq!(rows[1].state.status, MonoStatus::Idle);
        assert_eq!(
            serde_json::to_value(MonoStatus::NeedsYou).unwrap(),
            json!("needs-you")
        );
    }

    #[test]
    fn groups_show_what_members_asked_each_other_lately() {
        let now = 1_000_000_000;
        let entry = |id: &str, from: &str, to: &str, at: i64| Delegation {
            id: id.into(),
            from: from.into(),
            to: to.into(),
            message: id.into(),
            at,
            reply: None,
        };
        let delegations = vec![
            entry("1", "research", "docs", now - 1000),
            entry("2", "ivy", "outsider", now - 1000),
            entry("3", "docs", "research", now - 2 * 24 * 60 * 60 * 1000),
        ];
        let ids: Vec<&str> = group_exchanges(&nexa(), &delegations, now)
            .iter()
            .map(|item| item.id.as_str())
            .collect();
        assert_eq!(ids, vec!["1"]);
    }
}
