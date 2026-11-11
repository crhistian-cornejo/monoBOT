//! What a run of tool steps did, in words: one sentence per group ("Ran 3
//! commands and edited 2 files"), the icon its busiest kind wears, and the
//! files a turn changed with their line counts.
use super::{edit_preview, ToolCall, ToolState, VisualStep};
use std::collections::HashSet;

/// A group's sentence and the icon of the kind of step it did most.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub text: String,
    pub icon: &'static str,
}

/// One sentence for `tools`, in the order each kind of step first came.
/// While `live`, the kind the last step belongs to reads as happening now
/// ("Reading main.rs"), the others as done.
pub fn summarize(tools: &[ToolCall], live: bool) -> Summary {
    let mut order = Vec::new();
    let mut reads = HashSet::new();
    let mut edits = HashSet::new();
    let mut searches = 0;
    let mut read_calls = 0;
    let mut edit_calls = 0;
    let mut commands = 0;
    let mut agents = 0;
    let mut failed_agents = 0;
    let mut others = 0;
    let mut charts = 0;
    let mut chart_steps = 0;
    let mut current = "think";
    for tool in tools {
        let (action, target) = tool.summary();
        let visual = tool.visual_step();
        let kind = match action.as_str() {
            _ if visual.is_some() => "visual",
            "Read" | "List" | "Search" | "Find" => "research",
            "Edit" | "Write" => "edit",
            "Run" => "run",
            "Agent" => "agent",
            _ => "other",
        };
        if !order.contains(&kind) {
            order.push(kind);
        }
        if live {
            current = kind;
        }
        if let Some(step) = visual {
            chart_steps += 1;
            if step == VisualStep::Show {
                charts += 1;
            }
            continue;
        }
        match action.as_str() {
            "Read" => {
                read_calls += 1;
                reads.insert(target);
            }
            "Edit" | "Write" => {
                edit_calls += 1;
                edits.insert(target);
            }
            "Search" | "Find" | "List" => searches += 1,
            "Run" => commands += 1,
            "Agent" => {
                agents += 1;
                if matches!(tool.state, ToolState::Failed | ToolState::Cancelled) {
                    failed_agents += 1;
                }
            }
            _ => others += 1,
        }
    }
    if failed_agents > 0 {
        return Summary {
            text: if failed_agents == 1 {
                "Subagent failed".into()
            } else {
                format!("{failed_agents} subagents failed")
            },
            icon: "bot",
        };
    }
    let file_label = |paths: &HashSet<String>| {
        if paths.len() == 1 {
            let path = paths.iter().next().map(String::as_str).unwrap_or("");
            path.rsplit('/').next().unwrap_or("").to_owned()
        } else {
            format!("{} files", paths.len())
        }
    };
    let count = |n: usize, one: &str, many: &str| {
        if n == 1 {
            one.to_owned()
        } else {
            format!("{n} {many}")
        }
    };
    let parts: Vec<String> = order
        .iter()
        .map(|kind| {
            let live = *kind == current;
            let verb = |now: &str, done: &str| if live { now } else { done }.to_owned();
            match *kind {
                "research" if searches > 0 && !reads.is_empty() => {
                    verb("Exploring the project", "Explored the project")
                }
                "research" if !reads.is_empty() => {
                    format!("{} {}", verb("Reading", "Read"), file_label(&reads))
                }
                "research" => verb("Searching the project", "Searched the project"),
                "edit" => format!("{} {}", verb("Editing", "Edited"), file_label(&edits)),
                "run" => format!(
                    "{} {}",
                    verb("Running", "Ran"),
                    count(commands, "a command", "commands")
                ),
                "visual" if charts == 0 => verb("Planning a chart", "Planned a chart"),
                "visual" => format!(
                    "{} {}",
                    verb("Drawing", "Drew"),
                    count(charts, "a chart", "charts")
                ),
                "agent" => format!(
                    "{} {}",
                    verb("Running", "Ran"),
                    count(agents, "a subagent", "subagents")
                ),
                _ => format!(
                    "{} {}",
                    verb("Running", "Ran"),
                    count(others, "a tool", "tools")
                ),
            }
        })
        .collect();
    // React breaks equal call counts towards edit, run, agent, then research.
    let mut icon = "terminal";
    let mut most = 0;
    for (count, kind) in [
        (edit_calls, "pencil-line"),
        (commands, "terminal"),
        (agents, "bot"),
        (read_calls + searches, "search"),
        (others, "terminal"),
        (chart_steps, "monocode"),
    ] {
        if count > most {
            most = count;
            icon = kind;
        }
    }
    Summary {
        text: sentence(&parts),
        icon,
    }
}

/// "Ran 3 commands, read a.rs and edited 2 files": every clause after the
/// first starts in lower case, as one sentence does.
fn sentence(parts: &[String]) -> String {
    let mut out = String::new();
    for (ix, part) in parts.iter().enumerate() {
        if ix > 0 {
            out.push_str(if ix + 1 == parts.len() { " and " } else { ", " });
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                out.extend(first.to_lowercase());
                out.push_str(chars.as_str());
            }
        } else {
            out.push_str(part);
        }
    }
    out
}

/// A file a turn changed, with the lines its steps added and removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

/// The files `calls` changed, in the order they were first touched. Only
/// steps that finished count: a failed edit changed nothing.
pub fn changed_files(calls: &[ToolCall]) -> Vec<FileChange> {
    let mut files: Vec<FileChange> = Vec::new();
    for call in calls {
        if call.state != ToolState::Completed || !edit_preview::available(call) {
            continue;
        }
        let Some(preview) = edit_preview::edit_preview(call) else {
            continue;
        };
        if preview.path.is_empty() {
            continue;
        }
        match files.iter_mut().find(|file| file.path == preview.path) {
            Some(file) => {
                file.additions += preview.additions;
                file.deletions += preview.deletions;
            }
            None => files.push(FileChange {
                path: preview.path,
                additions: preview.additions,
                deletions: preview.deletions,
            }),
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(name: &str, input: serde_json::Value, state: ToolState) -> ToolCall {
        let mut call = ToolCall::new("1", name);
        call.input = input.to_string();
        call.state = state;
        call
    }

    fn done(name: &str, input: serde_json::Value) -> ToolCall {
        call(name, input, ToolState::Completed)
    }

    #[test]
    fn a_group_reads_as_one_sentence() {
        let tools = [
            done("Bash", json!({"command": "cargo test"})),
            done("Bash", json!({"command": "cargo fmt"})),
            done(
                "Edit",
                json!({"file_path": "/repo/a.rs", "old_string": "a", "new_string": "b"}),
            ),
            done(
                "Edit",
                json!({"file_path": "/repo/b.rs", "old_string": "a", "new_string": "b"}),
            ),
        ];
        assert_eq!(
            summarize(&tools, false),
            Summary {
                text: "Ran 2 commands and edited 2 files".into(),
                icon: "pencil-line",
            }
        );
        let three = [
            done("Read", json!({"file_path": "/repo/main.rs"})),
            done("Bash", json!({"command": "ls"})),
            done(
                "Edit",
                json!({"file_path": "/repo/a.rs", "old_string": "a", "new_string": "b"}),
            ),
        ];
        assert_eq!(
            summarize(&three, false).text,
            "Read main.rs, ran a command and edited a.rs"
        );
    }

    #[test]
    fn the_kind_at_work_reads_as_happening_now() {
        let tools = [
            done("Bash", json!({"command": "ls"})),
            call(
                "Read",
                json!({"file_path": "/repo/main.rs"}),
                ToolState::Running,
            ),
        ];
        assert_eq!(
            summarize(&tools, true).text,
            "Ran a command and reading main.rs"
        );
    }

    #[test]
    fn a_failed_subagent_is_said_first() {
        let mut agent = ToolCall::new("1", "Task");
        agent.input = json!({"description": "Look"}).to_string();
        agent.state = ToolState::Failed;
        agent.panel = Some(super::super::tasks::Panel::Agent(Default::default()));
        let summary = summarize(&[agent], false);
        assert_eq!(
            (summary.text.as_str(), summary.icon),
            ("Subagent failed", "bot")
        );
    }

    #[test]
    fn changed_files_add_up_per_file_and_skip_failed_edits() {
        let calls = [
            done(
                "Edit",
                json!({"file_path": "/repo/a.rs", "old_string": "a\nb", "new_string": "c"}),
            ),
            done(
                "Edit",
                json!({"file_path": "/repo/a.rs", "old_string": "x", "new_string": "y\nz"}),
            ),
            call(
                "Edit",
                json!({"file_path": "/repo/b.rs", "old_string": "x", "new_string": "y"}),
                ToolState::Failed,
            ),
            done(
                "Write",
                json!({"file_path": "/repo/new.rs", "content": "one\ntwo\n"}),
            ),
            done("Bash", json!({"command": "ls"})),
        ];
        let files = changed_files(&calls);
        assert_eq!(
            files
                .iter()
                .map(|f| (f.path.as_str(), f.additions, f.deletions))
                .collect::<Vec<_>>(),
            [("/repo/a.rs", 3, 3), ("/repo/new.rs", 2, 0)]
        );
    }
}
