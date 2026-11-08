//! The change an Edit or Write step made, from the step itself rather than
//! from git (`ToolDiffPreview.tsx`, `toolPreview` in the Tauri harnesses).
//!
//! Each provider describes an edit its own way: Claude's `Edit`,
//! `MultiEdit` and `Write` inputs, Codex's `fileChange` patches and ACP's
//! `diff` content (kept in the step's display as `diffs`).
use super::{ToolCall, ToolState};
use crate::diff::{build_unified_file, Line, LineKind};
use serde_json::Value;

/// Lines past this are left for the file itself.
pub const MAX_LINES: usize = 400;
/// Edits larger than this are not diffed in the transcript.
const MAX_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditPreview {
    pub path: String,
    pub lines: Vec<Line>,
    pub additions: usize,
    pub deletions: usize,
    /// Written content whose previous version is unknown.
    pub content_only: bool,
    /// Lines beyond [`MAX_LINES`] that were left out.
    pub hidden: usize,
}

impl EditPreview {
    /// What the preview shows, as the Tauri popover's heading said it.
    pub fn description(&self, state: &ToolState) -> &'static str {
        match state {
            ToolState::Running => "Proposed changes",
            ToolState::Failed | ToolState::Cancelled => "Attempted changes · tool did not complete",
            ToolState::Completed if self.content_only => {
                "Written content · previous contents unavailable"
            }
            ToolState::Completed => "Change preview",
        }
    }

    fn new(path: String, mut lines: Vec<Line>, content_only: bool) -> Option<Self> {
        if !lines
            .iter()
            .any(|line| matches!(line.kind, LineKind::Add | LineKind::Del))
        {
            return None;
        }
        let additions = lines.iter().filter(|l| l.kind == LineKind::Add).count();
        let deletions = lines.iter().filter(|l| l.kind == LineKind::Del).count();
        let hidden = lines.len().saturating_sub(MAX_LINES);
        lines.truncate(MAX_LINES);
        for line in &mut lines {
            line.text
                .truncate(line.text.trim_end_matches(['\n', '\r']).len());
        }
        Some(Self {
            path,
            lines,
            additions,
            deletions,
            content_only,
            hidden,
        })
    }
}

fn line(kind: LineKind, text: &str) -> Line {
    Line {
        kind,
        text: text.into(),
        old_number: None,
        new_number: None,
        chunk: None,
    }
}

/// `old` → `new` as changed lines with three lines of context.
fn replacement(old: &str, new: &str) -> Vec<Line> {
    if old.len() + new.len() > MAX_BYTES {
        return Vec::new();
    }
    let file = build_unified_file(old, new, crate::diff::CONTEXT_DEFAULT);
    let mut lines = Vec::new();
    for block in &file.blocks {
        if let crate::diff::Block::Hunk { lines: hunk, .. } = block {
            if !lines.is_empty() {
                lines.push(line(LineKind::Hunk, "⋯"));
            }
            lines.extend(hunk.iter().filter(|l| l.kind != LineKind::Hunk).cloned());
        }
    }
    lines
}

fn written(content: &str) -> Vec<Line> {
    if content.len() > MAX_BYTES {
        return Vec::new();
    }
    content
        .lines()
        .map(|text| line(LineKind::Add, text))
        .collect()
}

/// A patch as git or Codex prints it. Text without any patch markers is a
/// new file's content.
fn patch(text: &str) -> (Vec<Line>, bool) {
    let marked = text.lines().any(|l| {
        l.starts_with("@@")
            || (l.starts_with(['+', '-']) && !l.starts_with("+++ ") && !l.starts_with("--- "))
    });
    if !marked {
        return (written(text), true);
    }
    let lines = text
        .lines()
        .filter(|l| {
            !l.starts_with("+++ ")
                && !l.starts_with("--- ")
                && !l.starts_with("diff --git")
                && !l.starts_with("index ")
        })
        .map(|l| {
            if l.starts_with("@@") {
                line(LineKind::Hunk, l)
            } else if let Some(rest) = l.strip_prefix('+') {
                line(LineKind::Add, rest)
            } else if let Some(rest) = l.strip_prefix('-') {
                line(LineKind::Del, rest)
            } else {
                line(LineKind::Context, l.strip_prefix(' ').unwrap_or(l))
            }
        })
        .collect();
    (lines, false)
}

fn string<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value[*key].as_str())
}

/// Whether `call` says what it changed, without diffing anything: cheap
/// enough to ask while drawing.
pub fn available(call: &ToolCall) -> bool {
    if call
        .display
        .as_ref()
        .is_some_and(|display| display["diffs"].as_array().is_some_and(|d| !d.is_empty()))
    {
        return true;
    }
    if call.name.eq_ignore_ascii_case("fileChange") {
        return !call.output.trim().is_empty();
    }
    [
        "\"old_string\"",
        "\"oldString\"",
        "\"edits\"",
        "\"content\"",
        "\"contents\"",
        "\"patch\"",
    ]
    .iter()
    .any(|key| call.input.contains(key))
}

/// The change `call` made, or nothing when the step is not an edit or does
/// not say what it changed.
pub fn edit_preview(call: &ToolCall) -> Option<EditPreview> {
    let input = serde_json::from_str::<Value>(&call.input).unwrap_or(Value::Null);
    let path = string(&input, &["file_path", "filePath", "path", "notebook_path"])
        .unwrap_or("")
        .to_owned();
    let name = call.name.to_ascii_lowercase();
    // ACP: the provider's own diff content.
    if let Some(diffs) = call
        .display
        .as_ref()
        .and_then(|display| display["diffs"].as_array())
        .filter(|diffs| !diffs.is_empty())
    {
        let first = diffs[0]["path"].as_str().unwrap_or(&path).to_owned();
        let mut lines = Vec::new();
        let mut content_only = true;
        for diff in diffs {
            let new = diff["newText"].as_str().unwrap_or("");
            let changed = match diff["oldText"].as_str() {
                Some(old) => {
                    content_only = false;
                    replacement(old, new)
                }
                None => written(new),
            };
            if !lines.is_empty() && !changed.is_empty() {
                lines.push(line(LineKind::Hunk, diff["path"].as_str().unwrap_or("⋯")));
            }
            lines.extend(changed);
        }
        return EditPreview::new(first, lines, content_only);
    }
    if name == "filechange" {
        // Codex: each change's path on its own line, then its patch.
        let paths: Vec<&str> = input["paths"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        let mut sections: Vec<(&str, String)> = Vec::new();
        for text in call.output.lines() {
            if paths.contains(&text) {
                sections.push((text, String::new()));
            } else if let Some((_, body)) = sections.last_mut() {
                body.push_str(text);
                body.push('\n');
            }
        }
        let mut lines = Vec::new();
        let mut content_only = true;
        for (index, (section, body)) in sections.iter().enumerate() {
            let (changed, whole) = patch(body);
            content_only &= whole;
            if index > 0 && !changed.is_empty() {
                lines.push(line(LineKind::Hunk, section));
            }
            lines.extend(changed);
        }
        let first = sections
            .first()
            .map_or(path, |(path, _)| (*path).to_owned());
        return EditPreview::new(first, lines, content_only);
    }
    if let Some(edits) = input["edits"].as_array() {
        let mut lines = Vec::new();
        for edit in edits {
            let changed = replacement(
                string(edit, &["old_string", "oldString"]).unwrap_or(""),
                string(edit, &["new_string", "newString"]).unwrap_or(""),
            );
            if !lines.is_empty() && !changed.is_empty() {
                lines.push(line(LineKind::Hunk, "⋯"));
            }
            lines.extend(changed);
        }
        return EditPreview::new(path, lines, false);
    }
    if let (Some(old), Some(new)) = (
        string(&input, &["old_string", "oldString"]),
        string(&input, &["new_string", "newString"]),
    ) {
        return EditPreview::new(path, replacement(old, new), false);
    }
    if matches!(name.as_str(), "write" | "create" | "write_file") {
        if let Some(content) = string(&input, &["content", "contents", "text"]) {
            return EditPreview::new(path, written(content), true);
        }
    }
    if let Some(text) = string(&input, &["patch", "diff"]).filter(|_| name.contains("patch")) {
        let (lines, whole) = patch(text);
        return EditPreview::new(path, lines, whole);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(name: &str, input: Value) -> ToolCall {
        let mut call = ToolCall::new("t", name);
        call.input = input.to_string();
        call.state = ToolState::Completed;
        call
    }

    fn kinds(preview: &EditPreview) -> Vec<(LineKind, &str)> {
        preview
            .lines
            .iter()
            .map(|line| (line.kind, line.text.as_str()))
            .collect()
    }

    #[test]
    fn claude_edits_diff_their_strings() {
        let preview = edit_preview(&call(
            "Edit",
            serde_json::json!({"file_path":"/repo/a.rs","old_string":"fn a() {\n    1\n}\n","new_string":"fn a() {\n    2\n}\n"}),
        ))
        .unwrap();
        assert_eq!(preview.path, "/repo/a.rs");
        assert_eq!(
            kinds(&preview),
            vec![
                (LineKind::Context, "fn a() {"),
                (LineKind::Del, "    1"),
                (LineKind::Add, "    2"),
                (LineKind::Context, "}"),
            ]
        );
        assert_eq!((preview.additions, preview.deletions), (1, 1));
        assert_eq!(preview.description(&ToolState::Completed), "Change preview");
        assert_eq!(preview.description(&ToolState::Running), "Proposed changes");
    }

    #[test]
    fn multi_edits_and_writes() {
        let preview = edit_preview(&call(
            "MultiEdit",
            serde_json::json!({"file_path":"a","edits":[{"old_string":"a","new_string":"b"},{"old_string":"c","new_string":"d"}]}),
        ))
        .unwrap();
        assert_eq!(
            kinds(&preview),
            vec![
                (LineKind::Del, "a"),
                (LineKind::Add, "b"),
                (LineKind::Hunk, "⋯"),
                (LineKind::Del, "c"),
                (LineKind::Add, "d"),
            ]
        );
        let written = edit_preview(&call(
            "Write",
            serde_json::json!({"file_path":"b.md","content":"# Title\nBody\n"}),
        ))
        .unwrap();
        assert!(written.content_only);
        assert_eq!(written.additions, 2);
        assert_eq!(
            written.description(&ToolState::Completed),
            "Written content · previous contents unavailable"
        );
        assert_eq!(
            written.description(&ToolState::Failed),
            "Attempted changes · tool did not complete"
        );
    }

    #[test]
    fn codex_file_changes_split_by_path() {
        let mut change = call(
            "fileChange",
            serde_json::json!({"path":"/r/a.rs","paths":["/r/a.rs","/r/b.rs"]}),
        );
        change.output = "/r/a.rs\n@@ -1 +1 @@\n-old\n+new\n/r/b.rs\nfresh file\n".into();
        let preview = edit_preview(&change).unwrap();
        assert_eq!(preview.path, "/r/a.rs");
        assert_eq!(
            kinds(&preview),
            vec![
                (LineKind::Hunk, "@@ -1 +1 @@"),
                (LineKind::Del, "old"),
                (LineKind::Add, "new"),
                (LineKind::Hunk, "/r/b.rs"),
                (LineKind::Add, "fresh file"),
            ]
        );
        assert!(!preview.content_only);
    }

    #[test]
    fn acp_diff_content_is_used() {
        let mut edit = call("Edit file", serde_json::json!({}));
        edit.display =
            Some(serde_json::json!({"diffs":[{"path":"/r/x.ts","oldText":"a\n","newText":"b\n"}]}));
        let preview = edit_preview(&edit).unwrap();
        assert_eq!(preview.path, "/r/x.ts");
        assert_eq!(
            kinds(&preview),
            vec![(LineKind::Del, "a"), (LineKind::Add, "b")]
        );
    }

    #[test]
    fn availability_is_known_without_diffing() {
        assert!(available(&call(
            "Edit",
            serde_json::json!({"old_string":"a","new_string":"b"})
        )));
        assert!(available(&call(
            "Write",
            serde_json::json!({"content":"a"})
        )));
        assert!(!available(&call(
            "Read",
            serde_json::json!({"file_path":"a"})
        )));
        let mut codex = call("fileChange", serde_json::json!({"paths":["a"]}));
        assert!(!available(&codex));
        codex.output = "a\n+x".into();
        assert!(available(&codex));
    }

    #[test]
    fn non_edits_and_no_ops_have_no_preview() {
        assert!(edit_preview(&call("Read", serde_json::json!({"file_path":"a"}))).is_none());
        assert!(edit_preview(&call(
            "Edit",
            serde_json::json!({"file_path":"a","old_string":"same","new_string":"same"})
        ))
        .is_none());
        let long = "x\n".repeat(MAX_LINES + 5);
        let preview = edit_preview(&call(
            "Write",
            serde_json::json!({"file_path":"a","content":long}),
        ))
        .unwrap();
        assert_eq!(preview.lines.len(), MAX_LINES);
        assert_eq!(preview.hidden, 5);
    }
}
