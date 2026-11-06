//! Members' answers in the group's chat (`groupReplies.ts`): reading a
//! member's finished replies from its seat, posting them under its name, the
//! receipts of @Name messages, rounds and errands, and what the lead reads of
//! what went around it.
use super::{
    block::{Block, GroupMemberPost},
    clip, flat, js_prefix,
    mentions::{Named, starts_with_word},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// Members' answers post in the group's chat from this release on
/// (2026-11-05 18:00 UTC); older group.ask exchanges stay in the members'
/// own chats, so opening a group does not replay its whole history.
pub const GROUP_POSTS_SINCE: i64 = 1_793_901_600_000;

const REPLY_MAX_CHARS: usize = 20_000;

/// The receipt prefix of every message a group's chat sends a member.
pub fn group_request_prefix(group_session_id: &str) -> String {
    format!("app-{group_session_id}-")
}

/// A member's turn after the message at `start`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Turn {
    /// Its last written reply.
    pub text: String,
    /// The error it stopped on.
    pub error: Option<String>,
    /// Whether a later message closed it.
    pub finished: bool,
}

/// The member's turn after the message at `start`: its last written reply,
/// the error it stopped on, and whether a later message closed it.
pub fn turn_after(blocks: &[Block], start: usize) -> Turn {
    let mut turn = Turn::default();
    for block in blocks.iter().skip(start + 1) {
        if block.is_user_turn() {
            turn.finished = true;
            return turn;
        }
        if block.is_assistant()
            && block.tool.is_none()
            && !block.internal
            && !block.text.trim().is_empty()
        {
            turn.text = block.text.trim().to_string();
        }
        if block.notice.as_deref() == Some("error") && !block.text.trim().is_empty() {
            turn.error = Some(block.text.trim().to_string());
        }
    }
    turn
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupReply {
    pub request: String,
    pub text: String,
    pub at: i64,
}

/// The answers a member has finished for a group's chat: the last written
/// reply after each message the group sent it. A turn still running is not
/// finished; neither is a PASS, an empty answer, one sent before `since`
/// (normally [`GROUP_POSTS_SINCE`]) or one in `dropped`.
pub fn finished_group_replies(
    blocks: &[Block],
    busy: bool,
    group_session_id: &str,
    since: i64,
    dropped: &DroppedRequests,
) -> Vec<GroupReply> {
    let prefix = group_request_prefix(group_session_id);
    let mut replies = Vec::new();
    for (index, ask) in blocks.iter().enumerate() {
        let Some(request) = ask.app_request_id.as_deref() else {
            continue;
        };
        if !ask.is_user() || !request.starts_with(&prefix) || dropped.contains(request) {
            continue;
        }
        let turn = turn_after(blocks, index);
        if !turn.finished && busy {
            continue;
        }
        // A member that stopped on an error says so in the group, not silence.
        let answer = if !turn.text.is_empty() {
            turn.text
        } else if let Some(error) = turn.error {
            format!("Couldn't answer: {error}")
        } else {
            String::new()
        };
        let Some(at) = ask.started_at.or(ask.sent_at) else {
            continue;
        };
        if at < since || answer.is_empty() || answer == "PASS" {
            continue;
        }
        replies.push(GroupReply {
            request: request.to_string(),
            text: js_prefix(&answer, REPLY_MAX_CHARS).to_string(),
            at,
        });
    }
    replies
}

/// The requests whose answers the group's chat already shows.
pub fn posted_group_requests(blocks: &[Block]) -> HashSet<String> {
    blocks
        .iter()
        .filter_map(|block| block.group_member.as_ref().map(|post| post.request.clone()))
        .collect()
}

/// Who a post is from: its look and what it answered with.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PostAuthor {
    pub mono_id: String,
    pub name: String,
    pub mascot: String,
    pub color: String,
    pub harness: Option<String>,
    /// The model's display name.
    pub model: Option<String>,
}

/// A member's answer as the group's chat shows it.
pub fn group_member_post(author: &PostAuthor, reply: &GroupReply, id: &str, now: i64) -> Block {
    Block {
        id: id.into(),
        role: "assistant".into(),
        text: reply.text.clone(),
        group_member: Some(GroupMemberPost {
            mono_id: author.mono_id.clone(),
            name: author.name.clone(),
            mascot: author.mascot.clone(),
            color: author.color.clone(),
            request: reply.request.clone(),
            at: now,
            harness: author.harness.clone(),
            model: author.model.clone(),
        }),
        ..Block::default()
    }
}

/// The receipt of a message the user sent one member with @Name.
pub const DIRECT_PREFIX: &str = "group-direct-";
/// The receipt of a message every member answered before the lead.
pub const ROUND_PREFIX: &str = "group-round-";
/// The receipt of an errand the first member to do it answered.
pub const QUICK_PREFIX: &str = "group-quick-";

/// Whether a group chat's block is a message the members answered around
/// the lead: the user's @Name to one member, or an errand.
pub fn is_direct_ask(block: &Block) -> bool {
    block.is_user()
        && block.app_request_id.as_deref().is_some_and(|request| {
            request.starts_with(DIRECT_PREFIX) || request.starts_with(QUICK_PREFIX)
        })
}

const DROPPED_MAX: usize = 300;

/// Answers to errands that another member did first, kept out of the
/// group's chat. The desktop app keeps it in `localStorage` under
/// [`DroppedRequests::KEY`] as a JSON array of request ids, newest first;
/// the caller loads and saves it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DroppedRequests(pub Vec<String>);

impl DroppedRequests {
    pub const KEY: &'static str = "monocode.groupRequests.dropped";

    /// Reads the stored JSON; anything but an array of strings is empty.
    pub fn parse(raw: &str) -> Self {
        let value: serde_json::Value = serde_json::from_str(raw).unwrap_or_default();
        Self(
            value
                .as_array()
                .map(|entries| {
                    entries
                        .iter()
                        .filter_map(|entry| entry.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    /// Keeps a member's part of an errand out of the group's chat: another
    /// member did it first, and this one was stopped mid-way. False when
    /// there was nothing to drop, so nothing needs saving.
    pub fn drop_requests(&mut self, requests: &[String]) -> bool {
        if requests.is_empty() {
            return false;
        }
        let mut seen = HashSet::new();
        let mut next: Vec<String> = requests
            .iter()
            .chain(self.0.iter())
            .filter(|request| seen.insert(request.as_str()))
            .cloned()
            .collect();
        next.truncate(DROPPED_MAX);
        self.0 = next;
        true
    }

    pub fn contains(&self, request: &str) -> bool {
        self.0.iter().any(|known| known == request)
    }
}

/// The member a message opens with @Name for, and what to send it. Only one
/// member: a message for several, or for the lead, goes to the lead, who
/// splits it. Names may hold spaces, so the longest match wins.
pub fn direct_member<'a, T: Named>(text: &str, members: &'a [T]) -> Option<(&'a T, String)> {
    let trimmed = text.trim_start();
    if !trimmed.starts_with('@') {
        return None;
    }
    let lower = trimmed.to_lowercase();
    let mut named: Vec<&T> = members
        .iter()
        .filter(|entry| !entry.name().trim().is_empty())
        .collect();
    // Stable: of names as long, the first listed wins.
    named.sort_by_key(|entry| std::cmp::Reverse(super::js_len(entry.name())));
    let member = named.into_iter().find(|entry| {
        let name = format!("@{}", entry.name().trim().to_lowercase());
        lower.starts_with(&name) && !starts_with_word(&lower[name.len()..])
    })?;
    let skip = js_prefix(trimmed, super::js_len(member.name().trim()) + 1).len();
    let message = trimmed[skip..].trim().to_string();
    // "@Documents and @Asistente…" is for the lead to split.
    let lowered = message.to_lowercase();
    let others = members.iter().any(|entry| {
        !std::ptr::eq(entry, member)
            && !entry.name().trim().is_empty()
            && lowered.contains(&format!("@{}", entry.name().trim().to_lowercase()))
    });
    if message.is_empty() || others {
        return None;
    }
    Some((member, message))
}

/// A member's post the lead has not read, and the user's message it answers.
#[derive(Clone, Debug, PartialEq)]
pub struct DirectExchange<'a> {
    pub ask: Option<&'a str>,
    pub post: &'a Block,
}

/// What went around the lead in its chat since the user last wrote to it:
/// the @Name messages and errands and the members' answers to them. It saw
/// the answers to its own group.ask; these it reads with the user's next
/// message.
pub fn unseen_group_posts(blocks: &[Block]) -> Vec<DirectExchange<'_>> {
    let mut exchanges: Vec<DirectExchange<'_>> = Vec::new();
    let mut posts: Vec<&Block> = Vec::new();
    for block in blocks.iter().rev() {
        if is_direct_ask(block) {
            // Each goes in front, as the desktop app does: the latest post first.
            for post in posts.drain(..) {
                exchanges.insert(
                    0,
                    DirectExchange {
                        ask: Some(&block.text),
                        post,
                    },
                );
            }
            continue;
        }
        if block.is_user() && !block.draft && !block.internal {
            break;
        }
        if let Some(post) = &block.group_member {
            if post.request.contains("-direct-") || post.request.contains("-quick-") {
                posts.insert(0, block);
            }
        }
    }
    for post in posts {
        exchanges.insert(0, DirectExchange { ask: None, post });
    }
    exchanges
}

/// Tells the lead what the user and members said around it in its chat.
pub fn group_posts_context(exchanges: &[DirectExchange<'_>]) -> String {
    if exchanges.is_empty() {
        return String::new();
    }
    let lines: Vec<String> = exchanges
        .iter()
        .map(|exchange| {
            let asked = exchange
                .ask
                .map(|ask| format!("The user: \"{}\" → ", clip(&flat(ask), 300)))
                .unwrap_or_default();
            let name = exchange
                .post
                .group_member
                .as_ref()
                .map_or("", |post| post.name.as_str());
            format!(
                "- {asked}{name}: {}",
                clip(&flat(&exchange.post.text), 1200)
            )
        })
        .collect();
    format!(
        "<group_posts>\nThe user wrote to these members directly (with @Name, or an errand the first member able to did), and they answered in this chat; the user has read them. Build on them rather than repeating them, and save to memory what they decided.\n{}\n</group_posts>",
        lines.join("\n")
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::mono::groups::mentions::Person;
    use serde_json::json;

    const NOW: i64 = 1_793_923_200_000; // 2026-11-06

    pub(crate) fn user(id: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            role: "user".into(),
            text: text.into(),
            started_at: Some(NOW),
            ..Block::default()
        }
    }
    pub(crate) fn asked(id: &str, text: &str, request: &str) -> Block {
        Block {
            app_request_id: Some(request.into()),
            ..user(id, text)
        }
    }
    pub(crate) fn said(id: &str, text: &str) -> Block {
        Block {
            id: id.into(),
            role: "assistant".into(),
            text: text.into(),
            ..Block::default()
        }
    }
    fn look() -> PostAuthor {
        PostAuthor {
            mono_id: "docs".into(),
            name: "Documents".into(),
            mascot: "m".into(),
            color: "c".into(),
            ..PostAuthor::default()
        }
    }
    fn none() -> DroppedRequests {
        DroppedRequests::default()
    }

    #[test]
    fn groups_take_the_last_written_reply_to_each_group_message_once_it_is_done() {
        let blocks = vec![
            asked("u1", "<group_message>…", "app-g1-r1"),
            said("a1", "Let me look"),
            Block {
                tool: Some(json!({ "name": "Read" })),
                ..said("t1", "")
            },
            said("a2", "The PDF says 3 m/s"),
            user("u2", "something from the user"),
            said("a3", "Not for the group"),
            asked("u3", "<group_message>…", "app-g2-r9"),
            said("a4", "Other group"),
        ];
        assert_eq!(
            finished_group_replies(&blocks, false, "g1", GROUP_POSTS_SINCE, &none()),
            vec![GroupReply {
                request: "app-g1-r1".into(),
                text: "The PDF says 3 m/s".into(),
                at: NOW
            }]
        );
    }

    #[test]
    fn groups_wait_for_a_running_turn_and_leave_pass_empty_and_old_answers_out() {
        let blocks = vec![asked("u1", "q", "app-g1-r1"), said("a1", "Half an answer")];
        assert_eq!(
            finished_group_replies(&blocks, true, "g1", GROUP_POSTS_SINCE, &none()),
            vec![]
        );
        assert_eq!(
            finished_group_replies(&blocks, false, "g1", GROUP_POSTS_SINCE, &none()).len(),
            1
        );
        let pass = vec![asked("u1", "q", "app-g1-r1"), said("a", "PASS")];
        assert_eq!(
            finished_group_replies(&pass, false, "g1", GROUP_POSTS_SINCE, &none()),
            vec![]
        );
        let old = vec![
            Block {
                started_at: Some(1),
                ..asked("u1", "q", "app-g1-r1")
            },
            said("a", "Old"),
        ];
        assert_eq!(
            finished_group_replies(&old, false, "g1", GROUP_POSTS_SINCE, &none()),
            vec![]
        );
    }

    #[test]
    fn groups_say_when_a_member_stopped_on_an_error() {
        let blocks = vec![
            asked("u1", "q", "app-g1-r1"),
            Block {
                role: "system".into(),
                notice: Some("error".into()),
                ..said("e", "Usage limit")
            },
        ];
        let replies = finished_group_replies(&blocks, false, "g1", GROUP_POSTS_SINCE, &none());
        assert_eq!(replies[0].text, "Couldn't answer: Usage limit");
    }

    #[test]
    fn groups_post_each_request_once_under_the_members_name() {
        let reply = GroupReply {
            request: "app-g1-r1".into(),
            text: "Done".into(),
            at: NOW,
        };
        let post = group_member_post(&look(), &reply, "p1", NOW);
        assert_eq!(post.role, "assistant");
        assert_eq!(post.text, "Done");
        let member = post.group_member.as_ref().unwrap();
        assert_eq!(
            (member.name.as_str(), member.request.as_str()),
            ("Documents", "app-g1-r1")
        );
        assert_eq!(
            posted_group_requests(&[post]),
            HashSet::from(["app-g1-r1".to_string()])
        );
    }

    fn members() -> Vec<Person> {
        vec![
            Person::new("a", "Asistente General"),
            Person::new("d", "Documents"),
        ]
    }

    #[test]
    fn groups_find_the_one_member_a_message_opens_with_spaces_and_case_aside() {
        let members = members();
        let (member, message) =
            direct_member("@asistente general busca el correo", &members).unwrap();
        assert_eq!((member, message.as_str()), (&members[0], "busca el correo"));
        assert_eq!(
            direct_member("@Documents, resume el PDF", &members)
                .unwrap()
                .0
                .id,
            "d"
        );
    }

    #[test]
    fn groups_leave_to_the_lead_what_names_several_no_one_or_only_part_of_a_name() {
        let members = members();
        assert_eq!(
            direct_member("@Documents y @Asistente General, comparen", &members),
            None
        );
        assert_eq!(direct_member("hola @Documents", &members), None);
        assert_eq!(direct_member("@Documentsx hola", &members), None);
        assert_eq!(direct_member("@Documents", &members), None);
    }

    #[test]
    fn groups_tell_the_lead_what_went_around_it_since_the_user_last_wrote_to_it() {
        let reply = GroupReply {
            request: "app-g1-direct-k1".into(),
            text: "Resumen: 3 m/s".into(),
            at: NOW,
        };
        let blocks = vec![
            user("u0", "Hola"),
            said("l0", "Hola"),
            asked("u1", "@Documents resume el PDF", "group-direct-k1"),
            group_member_post(&look(), &reply, "p1", NOW),
        ];
        let exchanges = unseen_group_posts(&blocks);
        assert_eq!(exchanges.len(), 1);
        let context = group_posts_context(&exchanges);
        assert!(context.contains("The user: \"@Documents resume el PDF\""));
        assert!(context.contains("Documents: Resumen: 3 m/s"));
        assert_eq!(group_posts_context(&[]), "");
    }

    #[test]
    fn groups_hand_the_lead_posts_in_the_order_the_desktop_app_does() {
        let post = |id: &str, request: &str| {
            let reply = GroupReply {
                request: request.into(),
                text: id.into(),
                at: NOW,
            };
            group_member_post(&look(), &reply, id, NOW)
        };
        let blocks = vec![
            post("early", "app-g1-direct-k0"),
            asked("u1", "@Documents uno", "group-direct-k1"),
            post("p1", "app-g1-direct-k1"),
            post("p2", "app-g1-direct-k1"),
        ];
        let order: Vec<&str> = unseen_group_posts(&blocks)
            .iter()
            .map(|e| e.post.id.as_str())
            .collect();
        assert_eq!(order, vec!["early", "p2", "p1"]);
    }

    #[test]
    fn groups_tell_the_lead_about_an_errand_the_first_member_did() {
        let reply = GroupReply {
            request: "app-g1-quick-k3-docs".into(),
            text: "`D:/x/Informe.pdf`".into(),
            at: NOW,
        };
        let blocks = vec![
            asked("u1", "Pásame la ruta del informe", "group-quick-k3"),
            group_member_post(&look(), &reply, "p1", NOW),
        ];
        let context = group_posts_context(&unseen_group_posts(&blocks));
        assert!(context.contains("The user: \"Pásame la ruta del informe\""));
        assert!(context.contains("Documents: `D:/x/Informe.pdf`"));
    }

    #[test]
    fn groups_keep_a_member_stopped_on_an_errand_out_of_the_groups_chat() {
        let blocks = vec![
            asked("a1", "<group_message>", "app-g1-quick-k4-asis"),
            said("r1", "Voy a buscarlo…"),
        ];
        let mut dropped = DroppedRequests::parse("not json");
        assert_eq!(
            finished_group_replies(&blocks, false, "g1", 0, &dropped).len(),
            1
        );
        assert!(dropped.drop_requests(&["app-g1-quick-k4-asis".into()]));
        assert_eq!(
            finished_group_replies(&blocks, false, "g1", 0, &dropped),
            vec![]
        );
        assert!(!dropped.drop_requests(&[]));
        let saved = serde_json::to_string(&dropped).unwrap();
        assert_eq!(saved, "[\"app-g1-quick-k4-asis\"]");
        assert_eq!(DroppedRequests::parse(&saved), dropped);
    }

    #[test]
    fn groups_keep_the_newest_dropped_requests_first() {
        let mut dropped = DroppedRequests(vec!["old".into(), "b".into()]);
        dropped.drop_requests(&["b".into(), "new".into()]);
        assert_eq!(dropped.0, vec!["b", "new", "old"]);
    }
}
