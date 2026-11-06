//! A group brings agents together in one room: any of the user's Monos, bots
//! and workstreams. The group's own chat is its leader, the Principal (IVY)
//! running the room: it talks with the user and brings members in with the
//! app action group.ask, each member answering from its own conversation.
//!
//! Like a project, it may also have folders opened to its members, files
//! they read for context, and answers the user gave about paths and links
//! outside those folders. Its instructions, memory and scheduled tasks are
//! the group chat's own soul, memory and habits.
use super::{js_len, js_prefix, keep_last};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::LazyLock,
};

/// The desktop roster's `group` of a Mono whose `kind` is `group`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Group {
    /// Mono ids of the members, in the order they were added.
    pub members: Vec<String>,
    /// What the group is for, shared with every member.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goal: Option<String>,
    /// Folders opened to every member: read only, or read and change.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub folders: Vec<Folder>,
    /// Files every member reads for background when it matters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,
    /// What the user answered when a member needed a path or link outside the
    /// folders, so nobody asks twice. A granted folder also joins `folders`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub answers: Vec<AccessAnswer>,
    /// Asked and not answered yet: the group chat shows each one.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requests: Vec<AccessRequest>,
    /// Each member's own conversation in this group, by member id: the same
    /// agent, its tools and model, with a history apart from its own chat.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub seats: BTreeMap<String, String>,
}

/// A folder opened to the members (a bot's `BotFolder`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Folder {
    pub path: String,
    /// May also change files there; never delete them.
    pub edit: bool,
}

impl Folder {
    pub fn new(path: &str, edit: bool) -> Self {
        Self {
            path: path.into(),
            edit,
        }
    }
}

/// read: may read; edit: may read and change; no: the user said no.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Read,
    Edit,
    No,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Edit => "edit",
            Access::No => "no",
        }
    }
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "read" => Some(Access::Read),
            "edit" => Some(Access::Edit),
            "no" => Some(Access::No),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessAnswer {
    /// A full path or an http(s) link.
    pub target: String,
    pub access: Access,
    #[serde(default)]
    pub at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AccessRequest {
    pub id: String,
    pub target: String,
    /// Who needs it, as the request names them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    /// Why, in one line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub at: i64,
}

/// A group's own Mono as the roster keeps it: its id, its chat (where the
/// Principal leads the room), its display name and the group.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupMono {
    pub id: String,
    pub session_id: Option<String>,
    /// Its display name (`monoLook(mono).name`).
    pub name: String,
    pub group: Group,
}

pub const GROUP_MAX_MEMBERS: usize = 8;
pub const GROUP_CONTEXT_MAX: usize = 20;
/// As many folders as a bot can be given (`BOT_FOLDERS_MAX`).
pub const FOLDERS_MAX: usize = 20;
const ANSWERS_MAX: usize = 100;
const REQUESTS_MAX: usize = 10;

fn text(value: Option<&Value>, max: usize) -> Option<String> {
    let value = value?.as_str()?.trim();
    (!value.is_empty()).then(|| js_prefix(value, max).to_string())
}

/// `typeof value === "number" ? value : 0`, as epoch ms.
fn number(value: Option<&Value>) -> i64 {
    value
        .and_then(|value| value.as_i64().or_else(|| value.as_f64().map(|n| n as i64)))
        .unwrap_or(0)
}

fn field<'a>(entry: Option<&'a Map<String, Value>>, key: &str) -> Option<&'a Value> {
    entry.and_then(|entry| entry.get(key))
}

static SYSTEM_FOLDER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:[a-z]:[\\/](?:windows|program files(?: \(x86\))?|programdata)|/(?:system|usr|bin|sbin|etc|library|private|var|opt))(?:[\\/]|$)",
    )
    .unwrap()
});
static FULL_PATH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:[a-z]:[\\/]|/)").unwrap());
static WHOLE_DISK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^(?:[a-z]:[\\/]?|/)$").unwrap());
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^https?://\S+$").unwrap());
static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^[a-z]:").unwrap());

/// Why a folder cannot be opened to an agent, if it cannot (bots'
/// `folderProblem`). Any folder can be read; a whole disk is too much, and
/// system folders are never changed.
pub fn folder_problem(path: &str, edit: bool) -> Option<&'static str> {
    let trimmed = path.trim();
    if !FULL_PATH.is_match(trimmed) {
        return Some("Use a full path");
    }
    if WHOLE_DISK.is_match(trimmed) {
        return Some("A whole disk is too much; pick a folder");
    }
    if edit && SYSTEM_FOLDER.is_match(trimmed) {
        return Some("System folders can be read but never changed");
    }
    None
}

fn parse_folders(value: Option<&Value>) -> Vec<Folder> {
    let Some(entries) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    let mut folders = Vec::new();
    for entry in entries {
        let Some(path) = entry.get("path").and_then(Value::as_str) else {
            continue;
        };
        let path = path.trim();
        let edit =
            entry.get("edit") == Some(&Value::Bool(true)) && folder_problem(path, true).is_none();
        if folder_problem(path, false).is_some() || !seen.insert(path_key(path)) {
            continue;
        }
        folders.push(Folder::new(path, edit));
    }
    folders.truncate(FOLDERS_MAX);
    folders
}

fn is_target(value: &str) -> bool {
    is_link(value) || folder_problem(value, false).is_none()
}

/// An http(s) link with no spaces.
pub fn is_link(value: &str) -> bool {
    LINK.is_match(value)
}

/// `parseGroup`: the group a roster entry holds, keeping only what it can
/// trust. None for anything but a JSON object (or array, as JavaScript reads it).
pub fn parse(value: &Value) -> Option<Group> {
    let entry = match value {
        Value::Object(entry) => Some(entry),
        // An array is an object to JavaScript: it holds none of the fields.
        Value::Array(_) => None,
        _ => return None,
    };
    let mut members: Vec<String> = Vec::new();
    if let Some(ids) = field(entry, "members").and_then(Value::as_array) {
        for id in ids.iter().filter_map(Value::as_str) {
            if !id.is_empty() && !members.iter().any(|known| known == id) {
                members.push(id.into());
            }
        }
    }
    members.truncate(GROUP_MAX_MEMBERS);
    let goal = text(field(entry, "goal"), 500);
    let folders = parse_folders(field(entry, "folders"));
    let mut context: Vec<String> = Vec::new();
    if let Some(paths) = field(entry, "context").and_then(Value::as_array) {
        for path in paths {
            if let Some(value) = text(Some(path), 1024) {
                if folder_problem(&value, false).is_none() && !context.contains(&value) {
                    context.push(value);
                }
            }
        }
    }
    context.truncate(GROUP_CONTEXT_MAX);
    let answers = field(entry, "answers")
        .and_then(Value::as_array)
        .map(|items| {
            let answers = items
                .iter()
                .filter_map(|item| {
                    let target = text(item.get("target"), 2048)?;
                    let access = Access::parse(item.get("access")?.as_str()?)?;
                    is_target(&target).then(|| AccessAnswer {
                        target,
                        access,
                        at: number(item.get("at")),
                    })
                })
                .collect();
            keep_last(answers, ANSWERS_MAX)
        })
        .unwrap_or_default();
    let requests = field(entry, "requests")
        .and_then(Value::as_array)
        .map(|items| {
            let requests = items
                .iter()
                .filter_map(|item| {
                    let id = text(item.get("id"), 64)?;
                    let target = text(item.get("target"), 2048)?;
                    is_target(&target).then(|| AccessRequest {
                        id,
                        target,
                        by: text(item.get("by"), 60),
                        reason: text(item.get("reason"), 300),
                        at: number(item.get("at")),
                    })
                })
                .collect();
            keep_last(requests, REQUESTS_MAX)
        })
        .unwrap_or_default();
    let seats = field(entry, "seats")
        .and_then(Value::as_object)
        .map(|seats| {
            seats
                .iter()
                .filter_map(|(member, seat)| {
                    let seat = seat.as_str()?;
                    (members.contains(member) && is_seat_id(seat))
                        .then(|| (member.clone(), seat.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Group {
        members,
        goal,
        folders,
        context,
        answers,
        requests,
        seats,
    })
}

/// `^[A-Za-z0-9_-]{1,128}$`: a session id the app made.
pub(crate) fn is_seat_id(value: &str) -> bool {
    (1..=128).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// A desktop roster entry's group, as `parseMono` reads it: a Mono of kind
/// `group` always has one, empty when what it holds cannot be read.
pub fn parse_entry(entry: &Value) -> Option<Group> {
    (entry.get("kind").and_then(Value::as_str) == Some("group"))
        .then(|| entry.get("group").and_then(parse).unwrap_or_default())
}

/// A path as folders compare it: forward slashes, no trailing one, and
/// case-blind for a drive letter path, as Windows is.
pub fn path_key(path: &str) -> String {
    let slashed = path.trim().replace('\\', "/");
    let slashed = slashed.trim_end_matches('/');
    if DRIVE.is_match(slashed) {
        slashed.to_lowercase()
    } else {
        slashed.to_string()
    }
}

/// Whether `path` is `folder` or inside it.
pub fn is_inside(path: &str, folder: &str) -> bool {
    let target = path_key(path);
    let root = path_key(folder);
    target == root || target.starts_with(&format!("{root}/"))
}

/// What the members may do with a path or link: through the group's folders
/// or a saved answer, or None when nobody has asked the user yet.
pub fn group_access_for(group: &Group, target: &str) -> Option<Access> {
    if is_link(target) {
        let link = target.trim();
        return group
            .answers
            .iter()
            .rev()
            .find(|answer| is_link(&answer.target) && same_link(&answer.target, link))
            .map(|answer| answer.access);
    }
    // The closest folder: the longest one holding it, the first of equals.
    let mut closest: Option<(&Folder, usize)> = None;
    for folder in group
        .folders
        .iter()
        .filter(|folder| is_inside(target, &folder.path))
    {
        let length = js_len(&path_key(&folder.path));
        if closest.is_none_or(|(_, longest)| length > longest) {
            closest = Some((folder, length));
        }
    }
    if let Some((folder, _)) = closest {
        return Some(if folder.edit {
            Access::Edit
        } else {
            Access::Read
        });
    }
    if group.context.iter().any(|path| is_inside(target, path)) {
        return Some(Access::Read);
    }
    group
        .answers
        .iter()
        .rev()
        .find(|answer| !is_link(&answer.target) && is_inside(target, &answer.target))
        .map(|answer| answer.access)
}

/// Links compare by site and path, without a trailing slash or fragment.
fn same_link(a: &str, b: &str) -> bool {
    fn key(link: &str) -> String {
        let link = link.split_once('#').map_or(link, |(before, _)| before);
        link.trim_end_matches('/').to_lowercase()
    }
    key(a) == key(b)
}

/// Adds a pending request, or returns the one already waiting for it.
pub fn with_access_request(group: &Group, request: AccessRequest) -> (Group, AccessRequest) {
    let wanted = path_key(&request.target);
    if let Some(waiting) = group
        .requests
        .iter()
        .find(|entry| path_key(&entry.target) == wanted)
    {
        return (group.clone(), waiting.clone());
    }
    let mut next = group.clone();
    next.requests.push(request.clone());
    next.requests = keep_last(next.requests, REQUESTS_MAX);
    (next, request)
}

/// The user's answer to a request: saved so nobody asks again, and a folder
/// the user opened joins the group's folders with that access.
pub fn answer_access_request(group: &Group, request_id: &str, access: Access, now: i64) -> Group {
    let Some(request) = group.requests.iter().find(|entry| entry.id == request_id) else {
        return group.clone();
    };
    let key = path_key(&request.target);
    let mut answers: Vec<AccessAnswer> = group
        .answers
        .iter()
        .filter(|answer| path_key(&answer.target) != key)
        .cloned()
        .collect();
    answers.push(AccessAnswer {
        target: request.target.clone(),
        access,
        at: now,
    });
    let opens_folder = access != Access::No
        && !is_link(&request.target)
        && folder_problem(&request.target, access == Access::Edit).is_none();
    let folders = if opens_folder {
        let mut folders: Vec<Folder> = group
            .folders
            .iter()
            .filter(|folder| path_key(&folder.path) != key)
            .cloned()
            .collect();
        folders.push(Folder::new(request.target.trim(), access == Access::Edit));
        folders.truncate(FOLDERS_MAX);
        folders
    } else {
        group.folders.clone()
    };
    Group {
        folders,
        answers: keep_last(answers, ANSWERS_MAX),
        requests: group
            .requests
            .iter()
            .filter(|entry| entry.id != request_id)
            .cloned()
            .collect(),
        ..group.clone()
    }
}

/// What the user's answer says, as the group chat reads it.
pub fn access_answer_text(target: &str, access: Access) -> String {
    let what = if is_link(target) {
        "this link"
    } else {
        "this path"
    };
    match access {
        Access::No => format!("No: the group may not open {what}: {target}. Carry on without it."),
        Access::Edit => format!(
            "Yes, read and edit: the members may read and change files in {target} (never delete). Carry on."
        ),
        Access::Read => {
            format!("Yes, read only: the members may read {target} but not change it. Carry on.")
        }
    }
}

/// The folders and files the members may use, as their briefs list them.
pub fn group_places(group: &Group) -> String {
    let folders: Vec<String> = group
        .folders
        .iter()
        .map(|folder| {
            let access = if folder.edit {
                "read and edit, never delete"
            } else {
                "read only"
            };
            format!("- {} ({access})", folder.path)
        })
        .collect();
    let context: Vec<String> = group
        .context
        .iter()
        .map(|path| format!("- {path}"))
        .collect();
    let links: Vec<String> = group
        .answers
        .iter()
        .filter(|answer| is_link(&answer.target) && answer.access != Access::No)
        .map(|answer| format!("- {}", answer.target))
        .collect();
    let mut parts = vec![if folders.is_empty() {
        "No folders are open to the members.".to_string()
    } else {
        format!("Folders open to the members:\n{}", folders.join("\n"))
    }];
    if !context.is_empty() {
        parts.push(format!(
            "Context files, read them when they bear on the work:\n{}",
            context.join("\n")
        ));
    }
    if !links.is_empty() {
        parts.push(format!("Links the user allowed:\n{}", links.join("\n")));
    }
    parts.join("\n")
}

/// The access rule every member and the leader keep.
pub const GROUP_ACCESS_RULE: &str = "Stay inside the group's folders and files. Before you open a path or a link outside them, even one the user or another member sent, ask for it with app group.access {\"target\":\"<full path or link>\",\"reason\":\"<one line>\"}: it answers at once when the user already decided; otherwise the user is asked in the group chat and you end your turn until they answer. Never work around a \"no\".";

const MEMORY_LINES_MAX: usize = 60;
const MEMORY_CHARS_MAX: usize = 6_000;

static MEMORY_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[-*]\s").unwrap());

/// The project's live memory entries; the newest stay when it runs long.
pub fn project_memory(memory: Option<&str>) -> String {
    let lines: Vec<&str> = memory
        .unwrap_or_default()
        .split('\n')
        .map(str::trim)
        .filter(|line| MEMORY_LINE.is_match(line) && !line.contains("~~"))
        .collect();
    let mut lines = keep_last(lines, MEMORY_LINES_MAX);
    while lines.len() > 1 && js_len(&lines.join("\n")) > MEMORY_CHARS_MAX {
        lines.remove(0);
    }
    lines.join("\n")
}

const GROUP_ANSWER: &str = "Answer as yourself with your own contribution, briefly unless asked for more. You may ask another member of this group for something you need with app agents.message {\"to\":\"<member>\",\"message\":\"...\",\"wait\":true}, which waits for its reply. If you have nothing new to add, reply with exactly PASS.";

/// An errand goes to every member; the first to do it answers for all.
const QUICK_ANSWER: &str = "This is an errand for whichever member can do it fastest, not a discussion: every member got it, the first answer that does it goes to the user, and the others are stopped. If you can do it yourself right now, do just that and reply in one or two lines with the result, no preamble or summary. If it is not yours to do or you cannot, reply with exactly PASS at once.";

/// How a member hands the user a file, so the chat opens it on a click.
pub const FILE_LINK_RULE: &str = "When you point the user to a file, write its full path in backticks, e.g. `D:/Proyectos/Informe final.pdf`, never a bare path or only its name: the app turns it into a link that opens the file.";

/// What a member is told when the leader (or a round) brings it in: the
/// exact text its seat receives. `quick` marks an errand.
pub fn group_message(
    group_name: &str,
    goal: Option<&str>,
    message: &str,
    group: Option<&Group>,
    memory: Option<&str>,
    quick: bool,
) -> String {
    let remembered = project_memory(memory);
    let goal = goal
        .filter(|goal| !goal.is_empty())
        .map(|goal| format!("The group is for: {goal}\n"))
        .unwrap_or_default();
    let places = group
        .map(|group| format!("{}\n{GROUP_ACCESS_RULE}\n", group_places(group)))
        .unwrap_or_default();
    let remembered = if remembered.is_empty() {
        String::new()
    } else {
        format!(
            "What the project remembers (keep to these decisions and preferences):\n{remembered}\n"
        )
    };
    let answer = if quick { QUICK_ANSWER } else { GROUP_ANSWER };
    format!(
        "<group_message group=\"{group_name}\">\n{goal}{places}{remembered}{}\n</group_message>\n{answer} {FILE_LINK_RULE}",
        message.trim()
    )
}

/// The folders an agent's groups open to it, each once with the widest
/// access any of its groups gives, and the context files they share.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemberPlaces {
    pub folders: Vec<Folder>,
    pub context: Vec<String>,
}

pub fn member_places<'a>(groups: impl IntoIterator<Item = &'a Group>) -> MemberPlaces {
    let mut places = MemberPlaces::default();
    let mut folders: HashMap<String, usize> = HashMap::new();
    let mut context: HashMap<String, usize> = HashMap::new();
    for group in groups {
        for folder in &group.folders {
            match folders.get(&path_key(&folder.path)) {
                Some(&index) => places.folders[index].edit |= folder.edit,
                None => {
                    folders.insert(path_key(&folder.path), places.folders.len());
                    places.folders.push(folder.clone());
                }
            }
        }
        // A later group's spelling wins; the first one's place stays.
        for path in &group.context {
            match context.get(&path_key(path)) {
                Some(&index) => places.context[index] = path.clone(),
                None => {
                    context.insert(path_key(path), places.context.len());
                    places.context.push(path.clone());
                }
            }
        }
    }
    places
}

/// A group's members with the Principal first: it is in every group.
pub fn with_principal(members: &[String], principal: Option<&str>) -> Vec<String> {
    let rest = members
        .iter()
        .filter(|id| Some(id.as_str()) != principal)
        .cloned();
    let mut members: Vec<String> = principal
        .map(String::from)
        .into_iter()
        .chain(rest)
        .collect();
    members.truncate(GROUP_MAX_MEMBERS);
    members
}

/// The groups an agent is a member of.
pub fn groups_of<'a>(groups: &'a [GroupMono], mono_id: &str) -> Vec<&'a GroupMono> {
    groups
        .iter()
        .filter(|group| group.group.members.iter().any(|id| id == mono_id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mono::groups::brief::{Member, group_brief};
    use serde_json::json;

    fn project() -> Group {
        Group {
            members: vec!["a".into()],
            folders: vec![
                Folder::new("D:\\Projects\\1064", false),
                Folder::new("D:\\Projects\\1064\\Editables", true),
            ],
            context: vec!["C:/Users/me/brief.pdf".into()],
            ..Group::default()
        }
    }

    #[test]
    fn groups_keep_unique_member_ids_up_to_the_limit() {
        let ids: Vec<String> = (0..12).map(|index| format!("m{index}")).collect();
        let mut raw: Vec<Value> = ids.iter().map(|id| json!(id)).collect();
        raw.extend([json!("m1"), json!(4), json!("")]);
        let group = parse(&json!({ "members": raw, "goal": "  Plan  " })).unwrap();
        assert_eq!(group.members, ids[..GROUP_MAX_MEMBERS]);
        assert_eq!(group.goal.as_deref(), Some("Plan"));
        assert_eq!(parse(&Value::Null), None);
        assert_eq!(parse(&json!({})), Some(Group::default()));
        assert_eq!(
            serde_json::to_value(Group::default()).unwrap(),
            json!({ "members": [] })
        );
    }

    #[test]
    fn groups_frame_what_a_member_is_asked_and_let_it_pass() {
        let message = group_message(
            "Trip",
            Some("Plan the trip to Cusco"),
            " Find flights ",
            None,
            None,
            false,
        );
        assert!(message.contains("<group_message group=\"Trip\">"));
        assert!(message.contains("The group is for: Plan the trip to Cusco"));
        assert!(message.contains("Find flights"));
        assert!(message.contains("reply with exactly PASS"));
        assert!(message.contains("\"wait\":true"));
    }

    #[test]
    fn groups_tell_the_leader_how_to_run_the_room() {
        let group = Group {
            goal: Some("Plan the trip".into()),
            ..Group::default()
        };
        let brief = group_brief("Trip", &group, &[] as &[Member]);
        assert!(brief.contains("You lead the group \"Trip\", which is for: Plan the trip"));
        assert!(brief.contains("app group.ask"));
        assert!(brief.contains("@Name"));
        assert!(brief.contains("at most 3 rounds and 10 member messages"));
        assert!(brief.contains("no members yet"));
        assert!(brief.contains("group.access"));
        assert!(brief.contains("No folders are open to the members."));
    }

    #[test]
    fn groups_keep_folders_context_and_answers_they_can_trust() {
        let parsed = parse(&json!({
            "members": [],
            "folders": [
                { "path": "D:\\Docs", "edit": true },
                { "path": "d:/docs/", "edit": false },
                { "path": "relative", "edit": true },
                { "path": "C:\\Windows\\System32", "edit": true },
            ],
            "context": ["/home/me/a.pdf", "notes.txt"],
            "answers": [
                { "target": "https://example.com/a", "access": "read", "at": 1 },
                { "target": "/srv/x", "access": "maybe", "at": 1 },
            ],
            "requests": [{ "id": "r1", "target": "/srv/y", "by": "Research", "at": 2 }],
        }))
        .unwrap();
        assert_eq!(
            parsed.folders,
            vec![
                Folder::new("D:\\Docs", true),
                Folder::new("C:\\Windows\\System32", false)
            ]
        );
        assert_eq!(parsed.context, vec!["/home/me/a.pdf".to_string()]);
        assert_eq!(
            parsed.answers,
            vec![AccessAnswer {
                target: "https://example.com/a".into(),
                access: Access::Read,
                at: 1
            }]
        );
        assert_eq!(
            parsed.requests,
            vec![AccessRequest {
                id: "r1".into(),
                target: "/srv/y".into(),
                by: Some("Research".into()),
                reason: None,
                at: 2
            }]
        );
        // What it writes back reads the same in the desktop app.
        assert_eq!(
            serde_json::to_value(&parsed.requests).unwrap(),
            json!([{ "id": "r1", "target": "/srv/y", "by": "Research", "at": 2 }])
        );
    }

    #[test]
    fn groups_compare_paths_the_way_windows_does() {
        assert!(is_inside(
            "d:/projects/1064/planos/a.pdf",
            "D:\\Projects\\1064"
        ));
        assert!(!is_inside("D:\\Projects\\10645", "D:\\Projects\\1064"));
        assert!(!is_inside("/Users/me/Docs", "/users/me/docs"));
    }

    #[test]
    fn groups_answer_from_the_closest_folder_the_context_and_saved_answers() {
        let group = project();
        assert_eq!(
            group_access_for(&group, "D:\\Projects\\1064\\Planos\\p1.dwg"),
            Some(Access::Read)
        );
        assert_eq!(
            group_access_for(&group, "D:/Projects/1064/Editables/cap3.docx"),
            Some(Access::Edit)
        );
        assert_eq!(
            group_access_for(&group, "C:\\Users\\me\\brief.pdf"),
            Some(Access::Read)
        );
        assert_eq!(group_access_for(&group, "E:\\Other"), None);
        let answered = Group {
            answers: vec![
                AccessAnswer {
                    target: "E:\\Other".into(),
                    access: Access::No,
                    at: 1,
                },
                AccessAnswer {
                    target: "https://site.test/page/".into(),
                    access: Access::Read,
                    at: 1,
                },
            ],
            ..group
        };
        assert_eq!(
            group_access_for(&answered, "E:\\Other\\x.txt"),
            Some(Access::No)
        );
        assert_eq!(
            group_access_for(&answered, "https://site.test/page#top"),
            Some(Access::Read)
        );
        assert_eq!(group_access_for(&answered, "https://site.test/other"), None);
    }

    #[test]
    fn groups_ask_once_per_target_and_save_the_users_answer() {
        let group = project();
        let (first, _) = with_access_request(
            &group,
            AccessRequest {
                id: "r1".into(),
                target: "E:\\Survey".into(),
                by: Some("Documents".into()),
                at: 1,
                ..AccessRequest::default()
            },
        );
        let (again, request) = with_access_request(
            &first,
            AccessRequest {
                id: "r2".into(),
                target: "e:/survey/".into(),
                at: 2,
                ..AccessRequest::default()
            },
        );
        assert_eq!(request.id, "r1");
        assert_eq!(again.requests.len(), 1);

        let granted = answer_access_request(&again, "r1", Access::Edit, 5);
        assert!(granted.requests.is_empty());
        assert!(granted.folders.contains(&Folder::new("E:\\Survey", true)));
        assert_eq!(
            granted.answers,
            vec![AccessAnswer {
                target: "E:\\Survey".into(),
                access: Access::Edit,
                at: 5
            }]
        );
        assert_eq!(
            group_access_for(&granted, "E:\\Survey\\data.xlsx"),
            Some(Access::Edit)
        );

        let refused = answer_access_request(&first, "r1", Access::No, 6);
        assert_eq!(refused.folders, group.folders);
        assert_eq!(group_access_for(&refused, "E:\\Survey"), Some(Access::No));
        assert_eq!(
            answer_access_request(&group, "missing", Access::Read, 7),
            group
        );
    }

    #[test]
    fn groups_say_the_answer_in_words_the_group_chat_acts_on() {
        assert!(access_answer_text("E:\\Survey", Access::Read).contains("read only"));
        assert!(access_answer_text("E:\\Survey", Access::Edit).contains("never delete"));
        assert!(access_answer_text("https://x.test", Access::No).contains("this link"));
    }

    #[test]
    fn groups_give_a_member_each_folder_once_with_the_widest_access() {
        let other = Group {
            folders: vec![Folder::new("d:/projects/1064", true)],
            ..Group::default()
        };
        let places = member_places([&project(), &other]);
        assert_eq!(
            places.folders,
            vec![
                Folder::new("D:\\Projects\\1064", true),
                Folder::new("D:\\Projects\\1064\\Editables", true)
            ]
        );
        assert_eq!(places.context, vec!["C:/Users/me/brief.pdf".to_string()]);
    }

    #[test]
    fn groups_list_the_folders_and_the_access_rule_to_the_members() {
        let message = group_message(
            "Nexa",
            None,
            "Review chapter 3",
            Some(&project()),
            None,
            false,
        );
        assert!(message.contains("D:\\Projects\\1064 (read only)"));
        assert!(message.contains("Editables (read and edit, never delete)"));
        assert!(message.contains("C:/Users/me/brief.pdf"));
        assert!(message.contains("app group.access"));
    }

    #[test]
    fn groups_open_any_folder_to_read_but_never_a_disk_or_a_system_folder_to_change() {
        assert!(
            folder_problem("C:\\", false)
                .unwrap()
                .contains("whole disk")
        );
        assert!(folder_problem("Docs", false).unwrap().contains("full path"));
        let system = r"C:\Windows\System32";
        assert_eq!(folder_problem(system, false), None);
        assert!(
            folder_problem(system, true)
                .unwrap()
                .contains("never changed")
        );
        assert!(
            folder_problem("/etc/hosts", true)
                .unwrap()
                .contains("never changed")
        );
        assert_eq!(folder_problem("/Users/ana/Docs", true), None);
    }

    #[test]
    fn groups_keep_only_seats_of_current_members() {
        let group = parse(&json!({
            "members": ["docs"],
            "seats": { "docs": "docs-seat", "gone": "x", "bad": 4 },
        }))
        .unwrap();
        assert_eq!(
            group.seats,
            BTreeMap::from([("docs".to_string(), "docs-seat".to_string())])
        );
    }

    #[test]
    fn groups_read_a_roster_entry_as_the_desktop_app_does() {
        assert_eq!(
            parse_entry(&json!({ "kind": "bot", "group": { "members": ["a"] } })),
            None
        );
        assert_eq!(
            parse_entry(&json!({ "kind": "group", "group": 4 })),
            Some(Group::default())
        );
        let group =
            parse_entry(&json!({ "kind": "group", "group": { "members": ["a"] } })).unwrap();
        assert_eq!(group.members, vec!["a".to_string()]);
    }

    #[test]
    fn groups_put_the_principal_first_and_find_an_agents_groups() {
        let members = vec!["a".to_string(), "ivy".to_string(), "b".to_string()];
        assert_eq!(with_principal(&members, Some("ivy")), vec!["ivy", "a", "b"]);
        assert_eq!(with_principal(&members, None), members);
        let groups = vec![
            GroupMono {
                id: "g1".into(),
                group: Group {
                    members: members.clone(),
                    ..Group::default()
                },
                ..GroupMono::default()
            },
            GroupMono {
                id: "g2".into(),
                ..GroupMono::default()
            },
        ];
        let found: Vec<&str> = groups_of(&groups, "b")
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        assert_eq!(found, vec!["g1"]);
    }
}
