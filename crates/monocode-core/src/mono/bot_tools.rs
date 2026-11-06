//! What a bot may use and what its Claude session is launched with: the tool
//! half of src/features/bots/model/bots.ts (BotTools, botTools, parseBotTools,
//! deniedSkills, rulePath, folderProblem, botLaunch, isUserOnlyRole).
use super::bots;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

/// The services a bot can be given (BOT_SERVICES).
pub const SERVICES: [&str; 3] = ["gmail", "drive", "notion"];
/// Claude Code's own tools a bot may use (BOT_ACCESS).
pub const ACCESS: [&str; 4] = ["web", "read", "edit", "images"];
/// Web and reading files, unless its role says otherwise (DEFAULT_ACCESS).
pub const DEFAULT_ACCESS: [&str; 2] = ["web", "read"];
pub const FOLDERS_MAX: usize = 20;
/// The skill packs a bot can be given (BOT_SKILLS: Finance, then SKILL_PACKS).
pub const PLUGINS: [&str; 6] = [
    "monobot-finance",
    "monobot-design",
    "monobot-imagery",
    "monobot-dev",
    "monobot-audio",
    "monobot-openai",
];

/// A folder the user opened to a bot, beyond its home folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotFolder {
    pub path: String,
    /// May also change files there; never delete them.
    #[serde(default)]
    pub edit: bool,
}

/// A bot's own choice of tools, as the desktop roster's `botTools` stores it.
/// A missing list is its role's; for connectors and skills, a missing list on
/// the role too means every one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BotTools {
    pub services: Vec<String>,
    pub plugins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folders: Option<Vec<BotFolder>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connectors: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<String>>,
}

/// A bot's tools with its role's filled in (the result of botTools).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    pub services: Vec<String>,
    pub plugins: Vec<String>,
    pub access: Vec<String>,
    pub folders: Vec<BotFolder>,
    pub connectors: Option<Vec<String>>,
    pub skills: Option<Vec<String>>,
}

fn owned(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| (*s).to_owned()).collect()
}

/// The bot's own choice of tools when it has one, else its role's (botTools).
pub fn tools(role: &str, own: Option<&BotTools>) -> Tools {
    let info = bots::role(role)
        .or_else(|| bots::role("assistant"))
        .expect("the assistant role exists");
    Tools {
        services: own.map_or_else(|| owned(info.services), |o| o.services.clone()),
        plugins: own.map_or_else(|| owned(info.plugins), |o| o.plugins.clone()),
        access: own
            .and_then(|o| o.access.clone())
            .unwrap_or_else(|| owned(info.access.unwrap_or(&DEFAULT_ACCESS))),
        folders: own.and_then(|o| o.folders.clone()).unwrap_or_default(),
        connectors: own
            .and_then(|o| o.connectors.clone())
            .or_else(|| info.connectors.map(owned)),
        skills: own
            .and_then(|o| o.skills.clone())
            .or_else(|| info.skills.map(owned)),
    }
}

/// A skill name as Claude Code invokes it: `pdf`, `document-skills:xlsx`.
pub fn is_skill_name(value: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-'))
    };
    value.len() <= 120
        && match value.split_once(':') {
            Some((plugin, name)) => part(plugin) && part(name),
            None => part(value),
        }
}

/// `xlsx` covers `document-skills:xlsx`, so a hint holds whichever plugin ships it.
pub fn skill_matches(wanted: &str, installed: &str) -> bool {
    installed == wanted || installed.ends_with(&format!(":{wanted}"))
}

/// The installed skills a bot may not use: every one its list leaves out. A
/// bot without a list keeps them all.
pub fn denied_skills(allowed: Option<&[String]>, installed: &[String]) -> Vec<String> {
    let Some(allowed) = allowed else {
        return Vec::new();
    };
    installed
        .iter()
        .filter(|name| is_skill_name(name) && !allowed.iter().any(|w| skill_matches(w, name)))
        .cloned()
        .collect()
}

/// Why a folder cannot be opened to a bot, if it cannot (folderProblem). Any
/// folder can be read; a whole disk is too much, and system folders are
/// never changed.
pub fn folder_problem(path: &str, edit: bool) -> Option<&'static str> {
    let lower = path.trim().to_ascii_lowercase().replace('\\', "/");
    let bytes = lower.as_bytes();
    let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    if !((drive && bytes.get(2) == Some(&b'/')) || lower.starts_with('/')) {
        return Some("Use a full path");
    }
    let rest = if drive { &lower[2..] } else { &lower[..] };
    if rest.trim_matches('/').is_empty() {
        return Some("A whole disk is too much; pick a folder");
    }
    if edit {
        let first = rest.split('/').find(|p| !p.is_empty()).unwrap_or("");
        let system: &[&str] = if drive {
            &[
                "windows",
                "program files",
                "program files (x86)",
                "programdata",
            ]
        } else {
            &[
                "system", "usr", "bin", "sbin", "etc", "library", "private", "var", "opt",
            ]
        };
        if system.contains(&first) {
            return Some("System folders can be read but never changed");
        }
    }
    None
}

/// Keeps only what MonoBOT knows; anything else is dropped (parseBotTools).
pub fn parse(value: &Value) -> Option<BotTools> {
    let entry = value.as_object()?;
    let pick = |list: Option<&Value>, known: &[&str]| -> Vec<String> {
        let list = list.and_then(Value::as_array);
        known
            .iter()
            .filter(|k| list.is_some_and(|l| l.iter().any(|v| v.as_str() == Some(**k))))
            .map(|k| (*k).to_owned())
            .collect()
    };
    let strings = |key: &str| {
        entry.get(key).and_then(Value::as_array).map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect::<Vec<_>>()
        })
    };
    let folders = entry.get("folders").and_then(Value::as_array).map(|list| {
        list.iter()
            .filter_map(|folder| {
                let path = folder.get("path")?.as_str()?;
                let edit = folder.get("edit").and_then(Value::as_bool) == Some(true);
                folder_problem(path, edit).is_none().then(|| BotFolder {
                    path: path.to_owned(),
                    edit,
                })
            })
            .take(FOLDERS_MAX)
            .collect()
    });
    Some(BotTools {
        services: pick(entry.get("services"), &SERVICES),
        plugins: pick(entry.get("plugins"), &PLUGINS),
        access: entry
            .get("access")
            .filter(|v| v.is_array())
            .map(|v| pick(Some(v), &ACCESS)),
        folders,
        connectors: strings("connectors"),
        skills: strings("skills").map(|list| {
            let mut seen = HashSet::new();
            list.into_iter()
                .filter(|s| is_skill_name(s) && seen.insert(s.clone()))
                .collect()
        }),
    })
}

/// A folder as Claude Code's permission rules compare it, after `//`: its
/// POSIX form. On Windows the CLI compares `C:\Users\x` as `c/Users/x`.
pub fn rule_path(dir: &str) -> String {
    let path = dir.strip_prefix(r"\\?\").unwrap_or(dir).replace('\\', "/");
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return format!(
            "{}{}",
            (bytes[0] as char).to_ascii_lowercase(),
            path[2..].trim_end_matches('/')
        );
    }
    path.trim_matches('/').to_owned()
}

/// The soul's "never delete" made a rule Claude Code enforces: a bot moves
/// what it would remove into a _revisar folder instead, and never rewrites a
/// repository's history or pushes.
pub const DESTRUCTIVE_COMMANDS: [&str; 11] = [
    "Bash(rm:*)",
    "Bash(rmdir:*)",
    "Bash(del:*)",
    "Bash(erase:*)",
    "Bash(rd:*)",
    "Bash(Remove-Item:*)",
    "Bash(git push:*)",
    "Bash(git reset:*)",
    "Bash(git clean:*)",
    "Bash(format:*)",
    "Bash(shutdown:*)",
];
/// The Principal leads the bots rather than launching subagents of its own.
const LEAD_DENIED: [&str; 2] = ["Task", "Agent"];

/// The claude.ai connectors as Claude Code names them, as Echo reviewed them.
fn prefix(service: &str) -> &'static str {
    match service {
        "gmail" => "mcp__claude_ai_Gmail__",
        "drive" => "mcp__claude_ai_Google_Drive__",
        _ => "mcp__claude_ai_Notion__",
    }
}
fn read_tools(service: &str) -> &'static [&'static str] {
    match service {
        "gmail" => &["search_threads", "get_thread", "get_message", "list_labels"],
        "drive" => &[
            "search_files",
            "get_file_metadata",
            "read_file_content",
            "list_recent_files",
        ],
        _ => &[
            "search",
            "fetch",
            "create-pages",
            "update-page",
            "restore-pages",
            "create-database",
            "update-database",
            "update-data-source",
            "create-view",
            "update-view",
            "query-data-sources",
            "get-async-task",
        ],
    }
}
/// Tools that send, share or destroy: never available to a bot.
fn denied_service_tools(service: &str) -> &'static [&'static str] {
    match service {
        "gmail" => &[
            "send_message",
            "reply",
            "forward",
            "create_draft",
            "update_draft",
            "delete_draft",
            "trash_message",
            "trash_thread",
            "mark_message_spam",
            "mark_thread_spam",
            "delete_label",
        ],
        "drive" => &["share_file", "trash_file", "update_file", "copy_file"],
        _ => &["delete-pages", "move-pages"],
    }
}
/// Notion's tools carry a `notion-` prefix in some accounts.
fn tool_names(service: &str, tools: &[&str]) -> Vec<String> {
    tools
        .iter()
        .flat_map(|tool| {
            let mut names = vec![format!("{}{tool}", prefix(service))];
            if service == "notion" {
                names.push(format!("{}notion-{tool}", prefix(service)));
            }
            names
        })
        .collect()
}
fn access_tools(kind: &str) -> &'static [&'static str] {
    match kind {
        "web" => &["WebSearch", "WebFetch"],
        "read" => &["Read", "Glob", "Grep"],
        "edit" => &["Edit", "Write", "MultiEdit", "NotebookEdit"],
        // An app action, not a Claude tool: nothing to allow or deny.
        _ => &[],
    }
}

/// What MonoBOT adds to a bot's Claude session (botLaunch).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    pub plugins: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    /// Folders beyond its home, for `--add-dir`.
    pub add_dirs: Vec<String>,
    /// The app connectors it may use; none means every enabled one.
    pub connectors: Option<Vec<String>>,
    /// The installed skills it may use; none means every one.
    pub skills: Option<Vec<String>>,
}

/// `installed` are the skills Claude Code would load from the user's folders
/// and plugins; `connector_denied` the connector tools no bot may use
/// (connectorDeniedTools).
pub fn launch(
    role: &str,
    own: Option<&BotTools>,
    installed: &[String],
    connector_denied: &[String],
) -> Launch {
    let tools = tools(role, own);
    let can = |kind: &str| tools.access.iter().any(|a| a == kind);
    // Reading needs no asking: web pages and files under its home or the
    // folders the user opened. Changing files only where the user said so.
    let folder_rules = tools.folders.iter().flat_map(|folder| {
        let at = format!("//{}/**", rule_path(&folder.path));
        let mut rules = Vec::new();
        if can("read") {
            rules.push(format!("Read({at})"));
        }
        if can("edit") && folder.edit {
            rules.push(format!("Edit({at})"));
            rules.push(format!("Write({at})"));
        }
        rules
    });
    let mut allowed: Vec<String> = tools
        .services
        .iter()
        .flat_map(|s| tool_names(s, read_tools(s)))
        .collect();
    for kind in ["web", "read"] {
        if can(kind) {
            allowed.extend(owned(access_tools(kind)));
        }
    }
    allowed.extend(folder_rules);
    // Claude's own tools the bot was not given.
    let mut disallowed: Vec<String> = ["web", "read", "edit"]
        .into_iter()
        .filter(|kind| !can(kind))
        .flat_map(|kind| owned(access_tools(kind)))
        .collect();
    // Sending, sharing and deleting stay denied whatever the bot picks.
    disallowed.extend(
        tools
            .services
            .iter()
            .flat_map(|s| tool_names(s, denied_service_tools(s))),
    );
    // A service the role does not use stays out of the session entirely.
    disallowed.extend(
        SERVICES
            .iter()
            .filter(|s| !tools.services.iter().any(|own| own == *s))
            .map(|s| prefix(s).trim_end_matches("__").to_owned()),
    );
    disallowed.extend(owned(&DESTRUCTIVE_COMMANDS));
    disallowed.extend(connector_denied.iter().cloned());
    if role == "principal" {
        disallowed.extend(owned(&LEAD_DENIED));
    }
    // Installed skills outside its job stay closed to it.
    disallowed.extend(
        denied_skills(tools.skills.as_deref(), installed)
            .into_iter()
            .map(|name| format!("Skill({name})")),
    );
    Launch {
        plugins: tools.plugins.clone(),
        allowed_tools: allowed,
        disallowed_tools: disallowed,
        add_dirs: tools.folders.iter().map(|f| f.path.clone()).collect(),
        connectors: tools.connectors.clone(),
        skills: tools.skills.clone(),
    }
}

/// The skills Claude Code loads for a session in `cwd`: the user's and the
/// project's own (installedClaudeSkills). Empty when they cannot be listed,
/// which leaves every skill open.
pub fn installed_skills(cwd: &std::path::Path) -> Vec<String> {
    crate::skills::list_skills(cwd.to_string_lossy().into_owned(), None)
        .map(|skills| {
            skills
                .into_iter()
                .filter(|skill| skill.source == "claude" || skill.source == "agents")
                .map(|skill| skill.name)
                .collect()
        })
        .unwrap_or_default()
}

/// What a bot's session in `cwd` launches with (botLaunchForSession).
pub fn launch_for(mono: &super::Mono, cwd: &std::path::Path) -> Launch {
    launch(
        mono.bot_role.as_deref().unwrap_or("assistant"),
        mono.bot_tools.as_ref(),
        &installed_skills(cwd),
        &crate::connectors::connector_denied_tools(),
    )
}

/// Roles whose connectors reach something real (the user's phone): only the
/// user makes one, from the new-bot menu, never a bot through bots.create.
pub fn user_only(role: &str) -> bool {
    bots::role(role)
        .and_then(|r| r.connectors)
        .is_some_and(|connectors| connectors.contains(&"poki"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugins_match_the_web_build() {
        let packs = include_str!("../../../../src/features/skills/model/skillPacks.ts");
        for plugin in &PLUGINS[1..] {
            assert!(packs.contains(&format!("id: \"{plugin}\"")), "{plugin}");
        }
        let bots_ts = include_str!("../../../../src/features/bots/model/bots.ts");
        for command in DESTRUCTIVE_COMMANDS {
            assert!(bots_ts.contains(&format!("\"{command}\"")), "{command}");
        }
    }

    #[test]
    fn tools_fall_back_to_the_role() {
        let research = tools("research", None);
        assert_eq!(research.access, ["web", "read"]);
        assert!(research
            .connectors
            .unwrap()
            .contains(&"openalex".to_owned()));
        assert_eq!(research.skills.unwrap(), ["deep-research"]);
        let custom = tools("custom", None);
        assert!(custom.connectors.is_none() && custom.skills.is_none());
        let own = BotTools {
            services: vec!["gmail".into()],
            access: Some(vec!["read".into()]),
            ..Default::default()
        };
        let mine = tools("assistant", Some(&own));
        assert_eq!(mine.services, ["gmail"]);
        assert_eq!(mine.access, ["read"]);
    }

    #[test]
    fn skills_and_paths_follow_claude_code() {
        assert!(is_skill_name("pdf") && is_skill_name("document-skills:xlsx"));
        assert!(!is_skill_name("a b") && !is_skill_name("a:b:c"));
        assert!(skill_matches("xlsx", "document-skills:xlsx"));
        let installed = vec!["document-skills:xlsx".to_owned(), "pdf".to_owned()];
        assert_eq!(
            denied_skills(Some(&["xlsx".to_owned()]), &installed),
            ["pdf"]
        );
        assert!(denied_skills(None, &installed).is_empty());
        assert_eq!(rule_path(r"C:\Users\x\"), "c/Users/x");
        assert_eq!(rule_path("/home/x/"), "home/x");
        assert_eq!(folder_problem("relative", false), Some("Use a full path"));
        assert_eq!(
            folder_problem(r"C:\", false),
            Some("A whole disk is too much; pick a folder")
        );
        assert_eq!(
            folder_problem(r"C:\Windows\x", true),
            Some("System folders can be read but never changed")
        );
        assert_eq!(folder_problem(r"C:\Windows\x", false), None);
        assert_eq!(
            folder_problem("/usr/local", true),
            Some("System folders can be read but never changed")
        );
        assert_eq!(folder_problem(r"D:\dev\docs", true), None);
    }

    #[test]
    fn launch_denies_what_a_bot_was_not_given() {
        let principal = launch(
            "principal",
            None,
            &["pdf".to_owned()],
            &["mcp__poki__batch".to_owned()],
        );
        assert!(principal
            .allowed_tools
            .contains(&"mcp__claude_ai_Gmail__search_threads".to_owned()));
        for denied in [
            "mcp__claude_ai_Gmail__send_message",
            "Edit",
            "Bash(rm:*)",
            "Task",
            "Skill(pdf)",
            "mcp__poki__batch",
        ] {
            assert!(
                principal.disallowed_tools.contains(&denied.to_owned()),
                "{denied}"
            );
        }
        let inbox = launch("inbox", None, &[], &[]);
        assert!(inbox
            .disallowed_tools
            .contains(&"mcp__claude_ai_Notion".to_owned()));
        assert!(inbox.disallowed_tools.contains(&"WebFetch".to_owned()));
        assert!(!inbox.disallowed_tools.contains(&"Task".to_owned()));
        let own = BotTools {
            services: vec![],
            plugins: vec![],
            access: Some(vec!["read".into(), "edit".into()]),
            folders: Some(vec![BotFolder {
                path: r"D:\Docs".into(),
                edit: true,
            }]),
            ..Default::default()
        };
        let docs = launch("documents", Some(&own), &[], &[]);
        assert!(docs.allowed_tools.contains(&"Read(//d/Docs/**)".to_owned()));
        assert_eq!(docs.add_dirs, [r"D:\Docs"]);
        assert!(user_only("phone") && !user_only("research"));
    }

    #[test]
    fn parse_keeps_only_what_monobot_knows() {
        let value = serde_json::json!({
            "services": ["gmail", "slack"],
            "plugins": ["monobot-finance", "other"],
            "access": ["read", "fly"],
            "folders": [{"path": "C:\\Users\\x\\Docs", "edit": true}, {"path": "rel"}],
            "skills": ["pdf", "pdf", "bad name"],
        });
        let tools = parse(&value).unwrap();
        assert_eq!(tools.services, ["gmail"]);
        assert_eq!(tools.plugins, ["monobot-finance"]);
        assert_eq!(tools.access.unwrap(), ["read"]);
        assert_eq!(tools.folders.unwrap().len(), 1);
        assert_eq!(tools.skills.unwrap(), ["pdf"]);
        assert!(tools.connectors.is_none());
    }
}
