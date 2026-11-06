//! The desktop roster (`monocode:mono-roster`) as groups read it: every
//! agent a group can name, and each group's Mono, the way `listMonos()`
//! reads them (`parseMono`, `monoLook` and `withGroupLeads`).
use super::{
    brief::{Member, MemberKind},
    group::{parse_entry, with_principal, GroupMono},
};
use crate::project_appearance::MASCOTS;
use serde_json::Value;

/// Every agent and every group in the desktop roster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
    /// Every agent, groups included, in the roster's order.
    pub members: Vec<Member>,
    /// The groups, with the Principal first in each when there is one.
    pub groups: Vec<GroupMono>,
}

const BOT_ROLES: [&str; 13] = [
    "principal",
    "assistant",
    "inbox",
    "planner",
    "writer",
    "analyst",
    "finance",
    "documents",
    "research",
    "social",
    "images",
    "phone",
    "custom",
];

/// The name a Mono goes by until the user names it: "MonoCrab", "MonoShroom".
pub fn default_mono_name(mascot: &str) -> String {
    if mascot == "mushroom" {
        return "MonoShroom".into();
    }
    let mut chars = mascot.chars();
    let first = chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default();
    format!("Mono{first}{}", chars.as_str())
}

/// A project's last folder, as the rail names it without a label of its
/// own (`projectName`): `~` for the home folder, a bare drive as it is.
pub fn project_name(path: &str) -> String {
    if path.is_empty() {
        return "~".into();
    }
    let bytes = path.as_bytes();
    let drive = bytes.len() > 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/');
    let windows = drive || path.starts_with("\\\\") || path.starts_with("//");
    let slashed = if windows || (cfg!(windows) && !path.starts_with('/')) {
        path.replace('\\', "/")
    } else {
        path.to_string()
    };
    let trimmed = match slashed.trim_end_matches('/') {
        "" => "/",
        trimmed => trimmed,
    };
    let parts: Vec<&str> = trimmed.split('/').filter(|part| !part.is_empty()).collect();
    if trimmed == "~" || (parts.len() == 2 && (parts[0] == "Users" || parts[0] == "home")) {
        return "~".into();
    }
    let bytes = trimmed.as_bytes();
    if bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return trimmed.into();
    }
    parts.last().copied().unwrap_or(trimmed).to_string()
}

fn workstream(value: Option<&Value>) -> bool {
    let present = |key: &str| {
        value
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
    };
    present("project") && present("slug") && present("folder")
}

/// Reads the desktop roster JSON (an array of Monos). `project_name` names a
/// project path as the rail does: its label, else [`project_name`].
pub fn read_roster(entries: &Value, project_label: impl Fn(&str) -> String) -> Roster {
    let mut roster = Roster::default();
    for entry in entries.as_array().map(Vec::as_slice).unwrap_or_default() {
        let Some(id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        if id.is_empty()
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            continue;
        }
        let text = |key: &str| entry.get(key).and_then(Value::as_str);
        let mascot = text("mascot").unwrap_or(MASCOTS[0]);
        let name = text("name")
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| default_mono_name(mascot), String::from);
        let projects = entry
            .get("projects")
            .and_then(Value::as_array)
            .map(|paths| {
                paths
                    .iter()
                    .filter_map(Value::as_str)
                    .filter(|path| !path.is_empty())
                    .map(&project_label)
                    .collect()
            })
            .unwrap_or_default();
        let kind = match text("kind") {
            Some("bot") => MemberKind::Bot {
                role: text("botRole")
                    .filter(|role| BOT_ROLES.contains(role))
                    .unwrap_or("assistant")
                    .into(),
            },
            Some("workstream") if workstream(entry.get("workstream")) => MemberKind::Workstream,
            _ => MemberKind::Coding { projects },
        };
        if let Some(group) = parse_entry(entry) {
            roster.groups.push(GroupMono {
                id: id.into(),
                session_id: text("sessionId")
                    .filter(|id| !id.is_empty())
                    .map(String::from),
                name: name.clone(),
                group,
            });
        }
        roster.members.push(Member {
            id: id.into(),
            name,
            kind,
        });
    }
    // The Principal is in every group, also in ones made before it led them.
    if let Some(principal) = roster.members.iter().find(|member| member.is_principal()) {
        for group in &mut roster.groups {
            group.group.members = with_principal(&group.group.members, Some(&principal.id));
        }
    }
    roster
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn groups_read_the_desktop_roster_with_the_principal_leading_each_group() {
        let roster = read_roster(
            &json!([
                { "id": "ivy", "name": "IVY", "mascot": "crab", "kind": "bot", "botRole": "principal" },
                { "id": "docs", "mascot": "mushroom", "kind": "bot", "botRole": "nope" },
                { "id": "code", "name": " App ", "projects": ["D:/dev/app", "/srv/site/"] },
                { "id": "ws", "kind": "workstream", "workstream": { "project": "/p", "slug": "s", "folder": "/f" } },
                { "id": "bad id", "name": "Skipped" },
                {
                    "id": "nexa", "sessionId": "nexa-chat", "name": "Nexa", "kind": "group",
                    "group": { "members": ["docs", "code"], "goal": "Informe" }
                },
            ]),
            project_name,
        );
        let ids: Vec<&str> = roster
            .members
            .iter()
            .map(|member| member.id.as_str())
            .collect();
        assert_eq!(ids, vec!["ivy", "docs", "code", "ws", "nexa"]);
        assert_eq!(roster.members[1].name, "MonoShroom");
        assert_eq!(
            roster.members[1].kind,
            MemberKind::Bot {
                role: "assistant".into()
            }
        );
        assert_eq!(
            roster.members[2].kind,
            MemberKind::Coding {
                projects: vec!["app".into(), "site".into()]
            }
        );
        assert_eq!(roster.members[2].name, "App");
        assert_eq!(roster.members[3].kind, MemberKind::Workstream);
        let nexa = &roster.groups[0];
        assert_eq!((nexa.id.as_str(), nexa.name.as_str()), ("nexa", "Nexa"));
        assert_eq!(nexa.session_id.as_deref(), Some("nexa-chat"));
        assert_eq!(nexa.group.members, vec!["ivy", "docs", "code"]);
        assert_eq!(nexa.group.goal.as_deref(), Some("Informe"));
    }

    #[test]
    fn groups_name_unnamed_monos_and_projects_as_the_rail_does() {
        assert_eq!(default_mono_name("crab"), "MonoCrab");
        assert_eq!(project_name("C:\\Users\\me\\app\\"), "app");
        assert_eq!(project_name("D:"), "D:");
        assert_eq!(read_roster(&json!({}), project_name), Roster::default());
    }
}
