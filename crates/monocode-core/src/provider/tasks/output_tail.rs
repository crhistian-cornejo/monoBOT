//! The last lines a background command printed, read from the log its provider
//! keeps beside the session, so the chat can show it like a terminal (T3
//! Code's `CommandOutputPanel`). Read on the provider's thread, at most every
//! half second, and only the end of each file.
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// Lines kept per command.
pub const LINES: usize = crate::provider::OUTPUT_LINES;
/// Bytes read from the end of a log.
const WINDOW: u64 = 16 * 1024;
const LINE_CHARS: usize = 240;
const POLL: Duration = Duration::from_millis(500);
/// How much of Antigravity's transcript is searched for a command's log.
const TRANSCRIPT_WINDOW: u64 = 1024 * 1024;

#[derive(Default)]
pub(in crate::provider) struct Tails {
    logs: HashMap<String, Log>,
    next: Option<Instant>,
}

#[derive(Default)]
struct Log {
    /// The tool call that started it, to find its log in that call's result.
    source: Option<String>,
    path: Option<PathBuf>,
    seen: Option<(u64, SystemTime)>,
    lines: Vec<String>,
}

impl Tails {
    /// Whether the logs are due to be read again.
    pub fn due(&mut self) -> bool {
        let now = Instant::now();
        if self.next.is_some_and(|next| now < next) {
            return false;
        }
        self.next = Some(now + POLL);
        true
    }
    pub fn set_source(&mut self, id: &str, tool: &str) {
        self.logs.entry(id.into()).or_default().source = Some(tool.into());
    }
    pub fn source(&self, id: &str) -> Option<&str> {
        self.logs.get(id)?.source.as_deref()
    }
    pub fn set_path(&mut self, id: &str, path: PathBuf) {
        self.logs.entry(id.into()).or_default().path = Some(path);
    }
    pub fn has_path(&self, id: &str) -> bool {
        self.logs.get(id).is_some_and(|log| log.path.is_some())
    }
    /// Logs already matched to a command, so two commands never share one.
    pub fn paths(&self) -> Vec<PathBuf> {
        self.logs
            .values()
            .filter_map(|log| log.path.clone())
            .collect()
    }
    /// Forget finished commands and re-read the logs that changed.
    pub fn refresh(&mut self, running: &[String]) {
        self.logs.retain(|id, _| running.contains(id));
        for log in self.logs.values_mut() {
            let Some(path) = &log.path else { continue };
            let Ok(meta) = std::fs::metadata(path) else {
                continue;
            };
            let seen = (
                meta.len(),
                meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            );
            if log.seen == Some(seen) {
                continue;
            }
            log.seen = Some(seen);
            if let Some(lines) = read(path) {
                log.lines = lines;
            }
        }
    }
    pub fn lines(&self, id: &str) -> Vec<String> {
        self.logs
            .get(id)
            .map(|log| log.lines.clone())
            .unwrap_or_default()
    }
}

/// The last [`LINES`] printed lines of a log.
pub fn read(path: &Path) -> Option<Vec<String>> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(WINDOW);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(WINDOW).read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    // A window that starts mid-file starts mid-line.
    let text = if start > 0 {
        text.split_once('\n').map_or("", |(_, rest)| rest)
    } else {
        &text
    };
    Some(clean(text))
}

/// Terminal output as plain lines: colours dropped, a line rewritten with
/// `\r` (progress bars) kept at its last state, blank lines left out.
pub fn clean(text: &str) -> Vec<String> {
    let plain = strip_escapes(text);
    let mut lines: Vec<String> = plain
        .split('\n')
        .filter_map(|line| {
            let line = line.trim_end_matches('\r');
            let line = line.rsplit('\r').next().unwrap_or(line);
            let line: String = line
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .take(LINE_CHARS)
                .collect();
            let line = line.trim_end();
            (!line.trim().is_empty()).then(|| line.to_owned())
        })
        .collect();
    let excess = lines.len().saturating_sub(LINES);
    lines.drain(..excess);
    lines
}

fn strip_escapes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.next() {
            // CSI: parameters, then one final byte in @..~.
            Some('[') => {
                for c in chars.by_ref() {
                    if ('@'..='~').contains(&c) {
                        break;
                    }
                }
            }
            // OSC: up to BEL or ESC \.
            Some(']') => {
                while let Some(c) = chars.next() {
                    if c == '\u{7}' {
                        break;
                    }
                    if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Claude's backgrounded Bash result names its log: "Output is being written
/// to: /…/tasks/<id>.output. You will be notified…".
pub fn claude_output_file(result: &str) -> Option<PathBuf> {
    let (_, rest) = result.split_once("Output is being written to: ")?;
    let path = rest.split_whitespace().next()?.trim_end_matches('.');
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Antigravity's transcript records each command it sent to the background:
/// "Task Description: <command>\nTask logs are available at: file:///…log".
/// The newest entry for `command` whose log is not `taken` wins.
pub fn antigravity_log(transcript: &Path, command: &str, taken: &[PathBuf]) -> Option<PathBuf> {
    let mut file = std::fs::File::open(transcript).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TRANSCRIPT_WINDOW)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let command = command.trim();
    text.lines()
        .rev()
        .filter(|line| line.contains("Task logs are available at"))
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find_map(|entry| {
            let content = entry["content"].as_str()?;
            let mut description = None;
            let mut log = None;
            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("Task Description: ") {
                    description = Some(rest.trim());
                } else if let Some(rest) = line.strip_prefix("Task logs are available at: ") {
                    log = Some(file_url(rest.trim()));
                }
            }
            let log = log?;
            (description? == command && !taken.contains(&log)).then_some(log)
        })
}

fn file_url(url: &str) -> PathBuf {
    let path = url.strip_prefix("file://").unwrap_or(url);
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(byte) = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
            {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    PathBuf::from(String::from_utf8_lossy(&out).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_output_reads_as_its_last_plain_lines() {
        let text = "\u{1b}[1m\u{1b}[32m   Compiling\u{1b}[0m core\r\n\n\
                    Downloading 10%\rDownloading 55%\rDownloading 100%\n\
                    \u{1b}]0;title\u{7}tab\there\n";
        assert_eq!(
            clean(text),
            ["   Compiling core", "Downloading 100%", "tab here"]
        );
        let many: String = (1..=20).map(|n| format!("line {n}\n")).collect();
        let lines = clean(&many);
        assert_eq!(lines.len(), LINES);
        assert_eq!(lines.last().unwrap(), "line 20");
    }

    #[test]
    fn a_long_log_is_read_from_its_end() {
        let dir = std::env::temp_dir().join(format!("monocode-tail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("task.log");
        let body: String = (0..5_000)
            .map(|n| format!("Checking crate {n}\n"))
            .collect();
        std::fs::write(&path, body).unwrap();
        let lines = read(&path).unwrap();
        assert_eq!(lines.last().unwrap(), "Checking crate 4999");
        assert_eq!(lines.len(), LINES);

        let mut tails = Tails::default();
        tails.set_path("clippy", path.clone());
        tails.refresh(&["clippy".into()]);
        assert_eq!(tails.lines("clippy").last().unwrap(), "Checking crate 4999");
        std::fs::write(&path, "    Finished `dev` profile\n").unwrap();
        tails.refresh(&["clippy".into()]);
        assert_eq!(tails.lines("clippy"), ["    Finished `dev` profile"]);
        tails.refresh(&[]);
        assert!(
            tails.lines("clippy").is_empty(),
            "a finished command is forgotten"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn claude_names_the_log_in_its_bash_result() {
        assert_eq!(
            claude_output_file("Command running in background with ID: b10ox2bmv. Output is being written to: /private/tmp/claude-501/p/s/tasks/b10ox2bmv.output. You will be notified when it completes"),
            Some(PathBuf::from("/private/tmp/claude-501/p/s/tasks/b10ox2bmv.output"))
        );
        assert_eq!(claude_output_file("Exit code 0"), None);
    }

    #[test]
    fn antigravity_logs_are_found_by_their_command_in_its_transcript() {
        let dir = std::env::temp_dir().join(format!("monocode-agy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let transcript = dir.join("transcript.jsonl");
        let entry = |step: u32, command: &str| {
            serde_json::json!({"step_index":step,"source":"MODEL","type":"GENERIC","status":"RUNNING","content":format!("Created At: 2026-11-19T14:45:25-05:00\nTool is running as a background task with task id: s/task-{step}\nTask Description: {command}\nTask logs are available at: file:///Users/me/.gemini/antigravity-cli/brain/s/.system_generated/tasks/task-{step}.log\nLog output:\n    Checking aitrack-ai\r\n")}).to_string()
        };
        std::fs::write(
            &transcript,
            [
                entry(586, "cargo clippy --workspace"),
                r#"{"step_index":600,"content":"Made 1 replacement"}"#.into(),
                entry(610, "cargo clippy --workspace"),
                entry(612, "cargo test -p app"),
            ]
            .join("\n"),
        )
        .unwrap();
        let log = |step: u32| {
            PathBuf::from(format!(
                "/Users/me/.gemini/antigravity-cli/brain/s/.system_generated/tasks/task-{step}.log"
            ))
        };
        assert_eq!(
            antigravity_log(&transcript, "cargo clippy --workspace", &[]),
            Some(log(610)),
            "the newest run of the command"
        );
        assert_eq!(
            antigravity_log(&transcript, "cargo clippy --workspace", &[log(610)]),
            Some(log(586))
        );
        assert_eq!(
            antigravity_log(&transcript, " cargo test -p app ", &[]),
            Some(log(612))
        );
        assert_eq!(antigravity_log(&transcript, "npm run dev", &[]), None);
        assert_eq!(
            file_url("file:///Users/me/Application%20Support/task-1.log"),
            PathBuf::from("/Users/me/Application Support/task-1.log")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
