//! A group's chat as a board, so the Swarm view's Board, Members and Graph
//! show a team of personal agents the way they show a coding swarm: the
//! user's message reaches every member through the lead, each member's
//! answer reports back to it, and @names connect them. Pure: the caller
//! passes the chat's blocks and who is answering now.
use super::block::Block;
use crate::orchestration::board::{handle_from, report, Board, Kind, Member, State};
use std::collections::HashMap;

/// One agent of the group as the board shows it.
#[derive(Clone, Debug, Default)]
pub struct Seat {
    /// Its Mono id, as a member's post names it.
    pub id: String,
    pub name: String,
    /// Its conversation in this group, to open from the board.
    pub session: Option<i64>,
    /// Answering now.
    pub busy: bool,
}

fn member(seat: &Seat) -> Member {
    Member {
        session: seat.session,
        state: if seat.busy {
            State::Working
        } else {
            State::StandingBy
        },
        latest: if seat.busy {
            "Answering…".into()
        } else {
            String::new()
        },
        ..Default::default()
    }
}

/// The board of a group's chat. `lead` is the Principal, who has the last
/// word; `members` the agents it asks.
pub fn board(title: &str, lead: &Seat, members: &[Seat], blocks: &[Block]) -> Board {
    let mut first = member(lead);
    first.handle = Some(handle_from(&lead.name))
        .filter(|h| !h.is_empty() && h != "all")
        .unwrap_or_else(|| "lead".into());
    first.assignment = title.trim().to_owned();
    let lead_handle = first.handle.clone();
    let mut board = Board::team(title, first);
    let mut handles: HashMap<String, String> =
        HashMap::from([(lead.id.clone(), lead_handle.clone())]);
    for seat in members.iter().filter(|s| s.id != lead.id) {
        let handle = board.join(&seat.name, member(seat));
        handles.insert(seat.id.clone(), handle);
    }
    let mut at = 0u64;
    for block in blocks {
        if block.draft || block.internal {
            continue;
        }
        let when = block
            .group_member
            .as_ref()
            .map(|p| p.at)
            .or(block.started_at)
            .or(block.sent_at)
            .and_then(|t| u64::try_from(t).ok());
        if let Some(when) = when {
            at = at.max(when);
        }
        if let Some(post) = &block.group_member {
            let handle = match handles.get(&post.mono_id) {
                Some(handle) => handle.clone(),
                None => {
                    // Someone who has since left the group.
                    let handle = board.join(&post.name, Member::default());
                    handles.insert(post.mono_id.clone(), handle.clone());
                    handle
                }
            };
            if let Some(m) = board.members.iter_mut().find(|m| m.handle == handle) {
                m.provider = post.harness.clone().unwrap_or_default();
                m.model = post.model.clone();
            }
            board.record(
                &handle,
                &[&lead_handle],
                &report(&block.text),
                Kind::Report,
                at,
                false,
            );
        } else if block.is_user() {
            board.record(&lead_handle, &[], &block.text, Kind::Assign, at, true);
        } else if block.is_assistant() && block.tool.is_none() && block.agent_run.is_none() {
            board.record(
                &lead_handle,
                &[],
                &report(&block.text),
                Kind::Message,
                at,
                true,
            );
        }
    }
    board.live = lead.busy || members.iter().any(|s| s.busy);
    board
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::GroupMemberPost;

    fn seat(id: &str, name: &str, busy: bool) -> Seat {
        Seat {
            id: id.into(),
            name: name.into(),
            session: Some(id.len() as i64),
            busy,
        }
    }
    fn said(role: &str, text: &str, at: i64) -> Block {
        Block {
            role: role.into(),
            text: text.into(),
            started_at: Some(at),
            ..Default::default()
        }
    }
    fn answer(id: &str, name: &str, text: &str, at: i64) -> Block {
        Block {
            role: "assistant".into(),
            text: text.into(),
            group_member: Some(GroupMemberPost {
                mono_id: id.into(),
                name: name.into(),
                at,
                harness: Some("claude".into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_groups_chat_reads_as_a_board() {
        let lead = seat("principal", "Principal", false);
        let members = [
            seat("docs", "Documents", false),
            seat("research", "Research", true),
        ];
        let blocks = [
            said("user", "Plan the launch", 1_000),
            Block {
                internal: true,
                ..said("user", "hidden", 1_500)
            },
            answer(
                "docs",
                "Documents",
                "Draft ready.\n\nSee the brief; @research check the numbers.",
                2_000,
            ),
            answer("gone", "Old bot", "Done", 2_500),
            Block {
                tool: Some(serde_json::json!({"name":"Read"})),
                ..said("assistant", "reading", 2_600)
            },
            Block {
                role: "assistant".into(),
                text: "We launch on Friday.".into(),
                ..Default::default()
            },
        ];
        let board = board("Launch team", &lead, &members, &blocks);
        let handles: Vec<_> = board.members.iter().map(|m| m.handle.as_str()).collect();
        assert_eq!(handles, ["principal", "documents", "research", "old-bot"]);
        assert!(board.members[0].coordinator());
        assert!(board.live, "a member is answering");
        assert_eq!(board.members[2].state, State::Working);
        assert_eq!(board.posts.len(), 4, "internal turns and tools stay out");
        let ask = &board.posts[0];
        assert_eq!(
            (ask.kind, ask.from.as_str(), ask.all),
            (Kind::Assign, "principal", true)
        );
        let docs = &board.posts[1];
        assert_eq!(docs.kind, Kind::Report);
        assert_eq!(docs.to, ["principal", "research"]);
        assert_eq!(docs.text, "See the brief; @research check the numbers.");
        assert_eq!(docs.at, 2_000);
        assert_eq!(board.members[1].provider, "claude");
        assert_eq!(board.members[1].latest_seq, docs.seq);
        let last = board.posts.last().unwrap();
        assert_eq!((last.from.as_str(), last.all), ("principal", true));
        assert_eq!(
            last.at, 2_600,
            "a block without a time keeps the latest one"
        );
        // The graph counts the @research mention between members.
        let graph = board.graph(board.posts.len(), false);
        assert!(graph.edges.iter().any(|e| e.from == 1 && e.to == 2));
    }
}
