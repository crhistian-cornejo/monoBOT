//! A member's seat in a group (`groupSeats.ts`): its own conversation there.
//! It is the same agent (its soul, specialty, tools, model and permissions)
//! answering the group, with a history kept apart from its own chat, which
//! stays free for everything else. What it learns there goes to the group's
//! memory.
use super::{
    brief::Member,
    group::{Group, GroupMono},
};
use std::collections::HashSet;

/// The group and member whose seat a conversation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seat<'a> {
    pub group: &'a GroupMono,
    pub member: &'a Member,
}

/// The group and member whose seat this conversation is. `groups` are the
/// roster's groups and `roster` every agent, as `listMonos()` reads them.
pub fn seat_of<'a>(
    session_id: &str,
    groups: &'a [GroupMono],
    roster: &'a [Member],
) -> Option<Seat<'a>> {
    let (group, member_id) = seat_owner(session_id, groups)?;
    let member = roster.iter().find(|member| member.id == member_id)?;
    Some(Seat { group, member })
}

pub fn is_group_seat(session_id: &str, groups: &[GroupMono], roster: &[Member]) -> bool {
    seat_of(session_id, groups, roster).is_some()
}

/// The group whose seat this conversation is and the member sitting there,
/// found or not in the roster: what `monoForSession` reads after a Mono's
/// own chats, since a seat acts as its member.
pub fn seat_owner<'a>(
    session_id: &str,
    groups: &'a [GroupMono],
) -> Option<(&'a GroupMono, &'a str)> {
    groups.iter().find_map(|group| {
        group
            .group
            .seats
            .iter()
            .find(|(_, seat)| seat.as_str() == session_id)
            .map(|(member, _)| (group, member.as_str()))
    })
}

/// The conversation a member has in a group, when it has spoken there.
pub fn seat_session_id<'a>(group: &'a Group, member_id: &str) -> Option<&'a str> {
    group.seats.get(member_id).map(String::as_str)
}

/// Records the member's conversation in the group (`saveSeat`); the caller
/// saves the group. Reading it back keeps the seat only while the member is
/// in the group and the id is one the app made.
pub fn with_seat(group: &Group, member_id: &str, session_id: &str) -> Group {
    let mut next = group.clone();
    next.seats.insert(member_id.into(), session_id.into());
    next
}

/// Every seat conversation, for the lists that leave them out.
pub fn seat_session_ids(groups: &[GroupMono]) -> HashSet<String> {
    groups
        .iter()
        .flat_map(|group| group.group.seats.values().cloned())
        .collect()
}

/// What a member reads in its seat, ahead of the group's messages.
pub fn seat_brief(group_name: &str, member_name: &str) -> String {
    format!(
        "<group_seat>
You are {member_name}, in your own seat in the group \"{group_name}\". This conversation is yours only for this group: everything you write here shows in the group's chat under your name, and nothing here reaches your own chat with the user. Your soul, specialty, tools and model are the same as there. What you learn here about the project (decisions, preferences, people, files) goes to the group's memory: memory.add from this seat writes there; add \"personal\":true only for something about the user that holds beyond this group.
</group_seat>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::brief::MemberKind;

    fn roster() -> Vec<Member> {
        vec![Member {
            id: "docs".into(),
            name: "Documents".into(),
            kind: MemberKind::Bot {
                role: "documents".into(),
            },
        }]
    }

    fn nexa(group: Group) -> Vec<GroupMono> {
        vec![GroupMono {
            id: "nexa".into(),
            session_id: Some("nexa-chat".into()),
            name: "Nexa".into(),
            group,
        }]
    }

    #[test]
    fn groups_give_a_member_a_conversation_of_its_own() {
        let roster = roster();
        let group = Group {
            members: vec!["docs".into()],
            ..Group::default()
        };
        assert!(!is_group_seat("docs-seat", &nexa(group.clone()), &roster));
        let saved = with_seat(&group, "docs", "docs-seat");
        assert_eq!(seat_session_id(&saved, "docs"), Some("docs-seat"));
        let groups = nexa(saved);
        let seat = seat_of("docs-seat", &groups, &roster).unwrap();
        assert_eq!(
            (seat.group.id.as_str(), seat.member.id.as_str()),
            ("nexa", "docs")
        );
        // The seat acts as the member; its own chat is still its own.
        assert_eq!(
            seat_owner("docs-seat", &groups).map(|(_, member)| member),
            Some("docs")
        );
        assert_eq!(seat_of("docs-chat", &groups, &roster), None);
        assert_eq!(
            seat_session_ids(&groups),
            HashSet::from(["docs-seat".to_string()])
        );
        // A seat of an agent no longer in the roster is no one's.
        assert_eq!(seat_of("docs-seat", &groups, &[]), None);
    }

    #[test]
    fn groups_tell_the_member_where_its_words_go() {
        let brief = seat_brief("Nexa", "Documents");
        assert!(brief.contains("own seat in the group \"Nexa\""));
        assert!(brief.contains("\"personal\":true"));
    }
}
