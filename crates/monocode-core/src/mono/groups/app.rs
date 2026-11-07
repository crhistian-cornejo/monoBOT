//! The app actions a group's chat and its members call (`agentApp.ts`'s
//! `group.*` handlers): what they check, what they answer and what they
//! change, without sending anything. The host finds the sessions, sends the
//! messages and saves the group.
use super::{
    brief::{member_role, Member},
    group::{
        folder_problem, group_access_for, is_link, is_seat_id, with_access_request, Access,
        AccessRequest, Group, GroupMono,
    },
    js_len, js_prefix,
};
use serde_json::{json, Value};

/// The group actions and the fields each takes, for the app's validation.
pub const GROUP_ACTIONS: [(&str, &[&str]); 3] = [
    ("group.members", &[]),
    ("group.ask", &["member", "message", "wait"]),
    ("group.access", &["target", "reason", "group"]),
];

/// A trimmed, non-empty string of at most `max` UTF-16 units, or the
/// app's error naming the field.
pub fn required_string(value: Option<&Value>, name: &str, max: usize) -> Result<String, String> {
    match value.and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() && js_len(text) <= max => Ok(text.trim().into()),
        _ => Err(format!(
            "{name} must be a non-empty string under {max} characters"
        )),
    }
}

/// What a `group.access` call answers, and the group to save when it asked
/// the user (None when the answer was already known).
#[derive(Clone, Debug, PartialEq)]
pub struct AccessReply {
    pub reply: Value,
    pub group: Option<Group>,
}

/// The access a group has to a path or link, or asks the user for it
/// (`groupAccess`). `group` is the group's current state, `group_name` its
/// display name and `by` who asks; `request_id` is a new unique id.
pub fn group_access(
    group: &Group,
    group_name: &str,
    by: &str,
    input: &Value,
    request_id: &str,
    now: i64,
) -> Result<AccessReply, String> {
    let target = required_string(input.get("target"), "target", 2048)?;
    if !is_link(&target) && folder_problem(&target, false).is_some() {
        return Err("target must be a full path or an http(s) link".into());
    }
    if let Some(known) = group_access_for(group, &target) {
        let note = match known {
            Access::No => "The user said no: do not open it, and carry on without it.",
            Access::Edit => "Open: read it and change files there; never delete.",
            Access::Read => "Open to read; do not change it.",
        };
        return Ok(AccessReply {
            reply: json!({ "target": target, "access": known.as_str(), "note": note }),
            group: None,
        });
    }
    let reason = input
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|reason| !reason.is_empty())
        .map(|reason| js_prefix(reason, 300).to_string());
    let (next, asked) = with_access_request(
        group,
        AccessRequest {
            id: request_id.into(),
            target: target.clone(),
            by: Some(by.into()),
            reason,
            at: now,
        },
    );
    Ok(AccessReply {
        reply: json!({
            "target": target,
            "access": "asked",
            "requestId": asked.id,
            "note": format!("The user is asked in the group \"{group_name}\" whether the members may open it: read only, read and edit, or no. Do not open it now. Say in one line that you asked, and end your turn; the answer comes back to the group chat."),
        }),
        group: Some(next),
    })
}

/// The group a member asks for access through: its seat's group, else the
/// one group it is in, or the one `input.group` names by id or name.
/// `groups` are the seat's group alone when it asks from a seat, else
/// `groups_of(member)`; `is_member` whether the caller is a known agent.
pub fn access_group<'a>(
    groups: &[&'a GroupMono],
    is_member: bool,
    input: &Value,
) -> Result<&'a GroupMono, String> {
    let wanted = input
        .get("group")
        .and_then(Value::as_str)
        .map(|wanted| wanted.trim().to_lowercase())
        .unwrap_or_default();
    let target = if !wanted.is_empty() {
        groups
            .iter()
            .find(|group| group.id.to_lowercase() == wanted || group.name.to_lowercase() == wanted)
    } else if groups.len() == 1 {
        groups.first()
    } else {
        None
    };
    match target {
        Some(group) if is_member => Ok(group),
        _ if groups.len() > 1 => Err(format!(
            "You are in several groups; add \"group\":\"<name>\" ({})",
            groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => Err("Only a group or one of its members can ask for access".into()),
    }
}

/// The error for `group.members` and `group.ask` outside a group's chat.
pub const NOT_A_GROUP: &str = "Only a group's own chat can talk to its members";

/// What `group.members` answers: the members the lead may bring in.
pub fn members_reply(askable: &[&Member]) -> Value {
    json!({
        "members": askable
            .iter()
            .map(|member| json!({ "id": member.id, "name": member.name, "role": member_role(member) }))
            .collect::<Vec<_>>(),
    })
}

/// A `group.ask` call, checked: whom it asks, what, and the receipt its
/// message is sent with (`app-<group chat>-<request>`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupAsk<'a> {
    pub member: &'a Member,
    pub message: String,
    /// The answer comes back to this call rather than as a completion.
    pub wait: bool,
    pub app_request_id: String,
}

/// Checks a `group.ask` call from the group's chat `source_id`. The host
/// then sends `group_message(...)` with the project's memory to the
/// member's seat as `app_request_id`, notifying the group's chat unless
/// it waits.
pub fn group_ask<'a>(
    askable: &[&'a Member],
    source_id: &str,
    request_id: &str,
    input: &Value,
) -> Result<GroupAsk<'a>, String> {
    let wanted = required_string(input.get("member"), "member", 128)?.to_lowercase();
    let member = askable
        .iter()
        .find(|member| member.id.to_lowercase() == wanted || member.name.to_lowercase() == wanted)
        .ok_or("No member by that name or id; run group.members")?;
    if !is_seat_id(request_id) {
        return Err("Invalid request ID".into());
    }
    Ok(GroupAsk {
        member,
        message: required_string(input.get("message"), "message", 30_000)?,
        wait: input.get("wait") == Some(&Value::Bool(true)),
        app_request_id: format!("app-{source_id}-{request_id}"),
    })
}

/// Whose memory `memory.add` writes from a conversation: in a group's seat,
/// the group's, unless the fact is `personal`; anywhere else, its own.
pub fn memory_owner<'a>(
    own_id: &'a str,
    seat_group_id: Option<&'a str>,
    personal: bool,
) -> &'a str {
    match seat_group_id {
        Some(group) if !personal => group,
        _ => own_id,
    }
}

/// Whether two agents share a group: members of one group may ask each
/// other even while they answer someone.
pub fn share_a_group(groups: &[GroupMono], a: &str, b: &str) -> bool {
    groups.iter().any(|group| {
        let members = &group.group.members;
        members.iter().any(|id| id == a) && members.iter().any(|id| id == b)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::{
        brief::MemberKind,
        group::{AccessAnswer, Folder},
    };

    fn nexa() -> Group {
        Group {
            members: vec!["docs".into()],
            folders: vec![Folder::new("D:\\Projects\\1064", false)],
            answers: vec![AccessAnswer {
                target: "E:\\Other".into(),
                access: Access::No,
                at: 1,
            }],
            ..Group::default()
        }
    }

    #[test]
    fn groups_answer_access_they_know_and_ask_for_the_rest() {
        let group = nexa();
        let known = group_access(
            &group,
            "Nexa",
            "Documents",
            &json!({ "target": "D:/Projects/1064/a.pdf" }),
            "r1",
            5,
        )
        .unwrap();
        assert_eq!(known.group, None);
        assert_eq!(known.reply["access"], "read");
        assert_eq!(known.reply["note"], "Open to read; do not change it.");
        let refused = group_access(
            &group,
            "Nexa",
            "Documents",
            &json!({ "target": "E:\\Other\\x" }),
            "r1",
            5,
        )
        .unwrap();
        assert_eq!(
            refused.reply["note"],
            "The user said no: do not open it, and carry on without it."
        );

        let asked = group_access(
            &group,
            "Nexa",
            "Documents",
            &json!({ "target": " E:\\Survey ", "reason": "  the survey data " }),
            "r1",
            5,
        )
        .unwrap();
        assert_eq!(asked.reply["access"], "asked");
        assert_eq!(asked.reply["requestId"], "r1");
        assert!(asked.reply["note"]
            .as_str()
            .unwrap()
            .contains("asked in the group \"Nexa\""));
        let saved = asked.group.unwrap();
        assert_eq!(
            saved.requests,
            vec![AccessRequest {
                id: "r1".into(),
                target: "E:\\Survey".into(),
                by: Some("Documents".into()),
                reason: Some("the survey data".into()),
                at: 5,
            }]
        );
        // Asked again, the request already waiting answers.
        let again = group_access(
            &saved,
            "Nexa",
            "Research",
            &json!({ "target": "e:/survey" }),
            "r2",
            6,
        )
        .unwrap();
        assert_eq!(again.reply["requestId"], "r1");

        for bad in [json!({}), json!({ "target": "notes.txt" })] {
            assert!(group_access(&group, "Nexa", "Documents", &bad, "r", 1).is_err());
        }
        assert_eq!(
            group_access(&group, "Nexa", "D", &json!({ "target": "C:\\" }), "r", 1),
            Err("target must be a full path or an http(s) link".into())
        );
    }

    #[test]
    fn groups_pick_the_group_a_member_asks_through() {
        let group = |id: &str, name: &str| GroupMono {
            id: id.into(),
            name: name.into(),
            ..GroupMono::default()
        };
        let (nexa, trip) = (group("nexa", "Nexa"), group("trip", "Trip"));
        assert_eq!(access_group(&[&nexa], true, &json!({})).unwrap().id, "nexa");
        assert_eq!(
            access_group(&[&nexa, &trip], true, &json!({})),
            Err("You are in several groups; add \"group\":\"<name>\" (Nexa, Trip)".into())
        );
        assert_eq!(
            access_group(&[&nexa, &trip], true, &json!({ "group": " TRIP " }))
                .unwrap()
                .id,
            "trip"
        );
        assert_eq!(
            access_group(&[], true, &json!({})),
            Err("Only a group or one of its members can ask for access".into())
        );
        assert!(access_group(&[&nexa], false, &json!({})).is_err());
    }

    #[test]
    fn groups_check_what_the_lead_asks_a_member() {
        let docs = Member {
            id: "docs".into(),
            name: "Documents".into(),
            kind: MemberKind::Bot {
                role: "documents".into(),
            },
        };
        let askable = [&docs];
        assert_eq!(
            members_reply(&askable),
            json!({ "members": [{ "id": "docs", "name": "Documents", "role": "Documents bot" }] })
        );
        let ask = group_ask(
            &askable,
            "g1",
            "r1",
            &json!({ "member": "documents", "message": " Find it ", "wait": true }),
        )
        .unwrap();
        assert_eq!(
            (ask.member.id.as_str(), ask.message.as_str(), ask.wait),
            ("docs", "Find it", true)
        );
        assert_eq!(ask.app_request_id, "app-g1-r1");
        assert_eq!(
            group_ask(
                &askable,
                "g1",
                "r1",
                &json!({ "member": "ivy", "message": "x" })
            ),
            Err("No member by that name or id; run group.members".into())
        );
        assert_eq!(
            group_ask(
                &askable,
                "g1",
                "bad id",
                &json!({ "member": "docs", "message": "x" })
            ),
            Err("Invalid request ID".into())
        );
        assert_eq!(
            group_ask(&askable, "g1", "r1", &json!({ "member": "docs" })),
            Err("message must be a non-empty string under 30000 characters".into())
        );
    }

    #[test]
    fn groups_keep_what_a_seat_learns_in_the_group() {
        assert_eq!(memory_owner("docs", Some("nexa"), false), "nexa");
        assert_eq!(memory_owner("docs", Some("nexa"), true), "docs");
        assert_eq!(memory_owner("docs", None, false), "docs");
        let groups = vec![GroupMono {
            group: Group {
                members: vec!["docs".into(), "fin".into()],
                ..Group::default()
            },
            ..GroupMono::default()
        }];
        assert!(share_a_group(&groups, "docs", "fin"));
        assert!(!share_a_group(&groups, "docs", "ivy"));
    }
}
