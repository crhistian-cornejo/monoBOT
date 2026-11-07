//! A Mono chat's files kept together like folders, one per request: the
//! PDFs it downloaded, the Word or Excel files it wrote, and the files the
//! user handed it. Read from the chat itself, as the Tauri app's
//! `documentGroups` did, so a file shows up where it was asked for.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use crate::{
    tool::ToolCall,
    transcript::{Row, attachments},
};

/// The kinds of file a folder lists.
const FILE_EXTENSIONS: &str = "pdf|docx?|xlsx?|xlsm|pptx?|csv";
const TITLE_MAX: usize = 90;

// A path in a Markdown link's angle brackets or in backticks may hold spaces.
static QUOTED_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)(?:<|`)((?:[A-Za-z]:[\\/]|/|~/)[^<>`\n]+?\.(?:{FILE_EXTENSIONS}))(?:>|`)"
    ))
    .expect("a valid pattern")
});
static BARE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r#"(?im)(?:^|[\s("'\[])((?:[A-Za-z]:[\\/]|/|~/)[^\s<>"'`()\[\]|*?]+\.(?:{FILE_EXTENSIONS}))"#
    ))
    .expect("a valid pattern")
});
static FILE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"(?i)^(?:[A-Za-z]:[\\/]|/|~/)[^\n]+\.(?:{FILE_EXTENSIONS})$"
    ))
    .expect("a valid pattern")
});

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    /// Stable while the chat grows: the row its request starts at.
    pub id: String,
    pub title: String,
    /// When its request was sent.
    pub at: Option<u64>,
    /// Absolute paths, in the order the chat named them.
    pub files: Vec<String>,
}

/// Absolute paths of files with those extensions named in a text. A path
/// that starts at the home folder (`~/`) comes back whole.
pub fn file_paths_in(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for capture in QUOTED_PATH.captures_iter(text) {
        push(&mut found, &capture[1]);
    }
    for capture in BARE_PATH.captures_iter(text) {
        let path = capture.get(1).expect("the path group");
        // What a lookahead would check: the path ends where its sentence does.
        let next = text[path.end()..].chars().next();
        if next.is_none_or(|c| c.is_whitespace() || ")\"']].,;:".contains(c)) {
            push(&mut found, path.as_str());
        }
    }
    found
}

/// The chat's requests that left files behind, newest first.
pub fn folders(rows: &[Row]) -> Vec<Folder> {
    let mut folders: Vec<Folder> = Vec::new();
    let mut current: Option<Folder> = None;
    for (ix, row) in rows.iter().enumerate() {
        if row.first && row.is_prompt() {
            folders.extend(current.take().filter(|f| !f.files.is_empty()));
            current = Some(Folder {
                id: format!("request-{ix}"),
                title: request_title(&row.text),
                at: row.sent_at,
                files: Vec::new(),
            });
            continue;
        }
        let Some(folder) = current.as_mut() else {
            continue;
        };
        for path in row_files(row) {
            if !folder.files.contains(&path) {
                folder.files.push(path);
            }
        }
    }
    folders.extend(current.filter(|f| !f.files.is_empty()));
    folders.reverse();
    folders
}

/// The files a row wrote, downloaded, attached or named.
fn row_files(row: &Row) -> Vec<String> {
    let mut files = Vec::new();
    match row.role.as_str() {
        attachments::ROLE => {
            for path in attachments::paths(row).unwrap_or_default() {
                push(&mut files, &path.to_string_lossy());
            }
        }
        "tool" => {
            let Ok(call) = serde_json::from_str::<ToolCall>(&row.text) else {
                return files;
            };
            // A tool's answer is often JSON whose paths hold spaces.
            for text in [&call.input, &call.output] {
                match serde_json::from_str::<Value>(text) {
                    Ok(value) => json_paths(&value, &mut files),
                    Err(_) => {
                        for path in file_paths_in(text) {
                            push(&mut files, &path);
                        }
                    }
                }
            }
        }
        "assistant" => {
            for path in file_paths_in(&row.text) {
                push(&mut files, &path);
            }
        }
        _ => {}
    }
    files
}

fn json_paths(value: &Value, files: &mut Vec<String>) {
    match value {
        Value::String(text) if FILE_PATH.is_match(text.trim()) => push(files, text.trim()),
        // An MCP answer carries the tool's JSON as text.
        Value::String(text) if text.trim_start().starts_with(['{', '[']) => {
            match serde_json::from_str::<Value>(text) {
                Ok(inner) => json_paths(&inner, files),
                Err(_) => file_paths_in(text)
                    .iter()
                    .for_each(|path| push(files, path)),
            }
        }
        Value::String(text) => {
            for path in file_paths_in(text) {
                push(files, &path);
            }
        }
        Value::Array(items) => items.iter().for_each(|item| json_paths(item, files)),
        Value::Object(map) => map.values().for_each(|item| json_paths(item, files)),
        _ => {}
    }
}

fn push(files: &mut Vec<String>, raw: &str) {
    let raw = raw.trim();
    // A Markdown link writes a Windows path as </C:/...>.
    let raw = match raw.as_bytes() {
        [b'/', drive, b':', b'/' | b'\\', ..] if drive.is_ascii_alphabetic() => &raw[1..],
        _ => raw,
    };
    let path = match raw.strip_prefix("~/") {
        Some(rest) => match dirs::home_dir() {
            Some(home) => home.join(rest).to_string_lossy().into_owned(),
            None => return,
        },
        None => raw.to_owned(),
    };
    if !files.contains(&path) {
        files.push(path);
    }
}

/// The request's first line, short enough for a folder's name.
fn request_title(text: &str) -> String {
    let line = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    if line.is_empty() {
        return "Documents".into();
    }
    if line.chars().count() <= TITLE_MAX {
        return line.to_owned();
    }
    let cut: String = line.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(role: &str, text: &str, first: bool) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first,
            sent_at: first.then_some(7),
        }
    }

    fn tool(output: &str) -> String {
        let mut call = ToolCall::new("t1", "web.download");
        call.input = r#"{"url":"https://example.com/paper"}"#.into();
        call.output = output.into();
        serde_json::to_string(&call).unwrap()
    }

    #[test]
    fn texts_name_files_by_their_paths_with_or_without_spaces() {
        assert_eq!(
            file_paths_in(
                "Saved `/Users/a/Documents/MonoBOT/Research/airGR and teaching.pdf` and \
                 /tmp/report.docx. See </C:/Users/a/sheet.xlsx> or /tmp/notes.txt."
            ),
            [
                "/Users/a/Documents/MonoBOT/Research/airGR and teaching.pdf",
                "C:/Users/a/sheet.xlsx",
                "/tmp/report.docx",
            ]
        );
        let home = dirs::home_dir().unwrap();
        assert_eq!(
            file_paths_in("PDF en `~/Documents/MonoBOT/Research/A paper.pdf`."),
            [home
                .join("Documents/MonoBOT/Research/A paper.pdf")
                .to_string_lossy()]
        );
        assert!(file_paths_in("https://example.com/paper.pdf").is_empty());
    }

    #[test]
    fn each_request_keeps_the_files_it_left_newest_first() {
        let rows = [
            row("user", "Hola", true),
            row("assistant", "Hola, ¿qué buscas?", true),
            row("user", "Paper de diseño hidrológico con R\nde preferencia abiertos", true),
            row(
                "tool",
                &tool(r#"{"path":"/Users/a/Documents/MonoBOT/Research/airGR and teaching.pdf","url":"https://x","bytes":10}"#),
                true,
            ),
            row(
                "assistant",
                "PDF en `/Users/a/Documents/MonoBOT/Research/airGR and teaching.pdf` y /tmp/gamlss.pdf.",
                true,
            ),
            row(
                "tool",
                &tool(
                    &serde_json::json!({ "content": [{ "type": "text", "text":
                        r#"{"path":"/Users/a/Documents/MonoBOT/Research/Around and about.pdf","bytes":3}"#
                    }] })
                    .to_string(),
                ),
                true,
            ),
            row("user", "Resume esto", true),
            row(attachments::ROLE, &attachments::from_paths(&["/tmp/recibo.pdf".into()]).to_string(), true),
            row("assistant", "Listo.", true),
        ];
        let folders = folders(&rows);
        assert_eq!(
            folders,
            [
                Folder {
                    id: "request-6".into(),
                    title: "Resume esto".into(),
                    at: Some(7),
                    files: vec!["/tmp/recibo.pdf".into()],
                },
                Folder {
                    id: "request-2".into(),
                    title: "Paper de diseño hidrológico con R".into(),
                    at: Some(7),
                    files: vec![
                        "/Users/a/Documents/MonoBOT/Research/airGR and teaching.pdf".into(),
                        "/tmp/gamlss.pdf".into(),
                        "/Users/a/Documents/MonoBOT/Research/Around and about.pdf".into(),
                    ],
                },
            ],
            "a request with no files has no folder"
        );
    }

    #[test]
    fn a_long_request_names_its_folder_by_its_start() {
        let title = request_title(&"palabra ".repeat(30));
        assert_eq!(title.chars().count(), TITLE_MAX);
        assert!(title.ends_with('…'));
        assert_eq!(request_title("  \n"), "Documents");
    }
}
