//! What each agent did with the user's mail: which action, on which account or message, what
//! it searched, and whether it worked. One JSON line each in `mail/activity.jsonl`, the newest
//! last, kept to the last `KEPT`. It never holds a message's text or a password: only enough to
//! see who looked at what (above all a mailbox that is somebody else's).
use super::*;

const KEPT: usize = 1_000;
/// The file is cut back to `KEPT` lines once it grows past this.
const MAX_BYTES: u64 = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub at: i64,
    /// The agent's id; empty for one with no identity.
    pub agent: String,
    pub action: String,
    /// The account, message or search it was about, cut short.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub about: String,
    pub ok: bool,
}

fn path(data_dir: &Path) -> PathBuf {
    data_dir.join("mail").join("activity.jsonl")
}

/// What an action was about, from its input: never a message's text.
pub fn about(action: &str, input: &Value) -> String {
    let text = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
    };
    let about = match action {
        "mail.search" => {
            let account = text("account");
            if account.is_empty() {
                format!("search «{}»", text("query"))
            } else {
                format!("search «{}» in {account}", text("query"))
            }
        }
        "mail.read" | "mail.draft" => format!("message {}", text("id")),
        "mail.propose" if !text("id").is_empty() => format!("reply to {}", text("id")),
        "mail.propose" => format!("new message from {} to {}", text("account"), text("to")),
        "mail.connect" => text("address").to_owned(),
        "mail.list" => {
            let view = text("view");
            if view.is_empty() {
                "inbox".to_owned()
            } else {
                view.to_owned()
            }
        }
        _ => String::new(),
    };
    about.chars().take(200).collect()
}

/// Adds one line. A failure to write is not the agent's problem: it is ignored.
pub fn record(data_dir: &Path, entry: &Entry) {
    let file = path(data_dir);
    let _ = fs::create_dir_all(data_dir.join("mail"));
    if fs::metadata(&file).is_ok_and(|m| m.len() > MAX_BYTES) {
        let kept = recent(data_dir, KEPT);
        let text: String = kept
            .iter()
            .rev()
            .filter_map(|e| serde_json::to_string(e).ok())
            .map(|line| line + "\n")
            .collect();
        let _ = fs::write(&file, text);
    }
    if let Ok(line) = serde_json::to_string(entry) {
        use std::io::Write;
        if let Ok(mut out) = fs::OpenOptions::new().create(true).append(true).open(&file) {
            let _ = writeln!(out, "{line}");
        }
    }
}

/// The newest `limit` entries, newest first.
pub fn recent(data_dir: &Path, limit: usize) -> Vec<Entry> {
    let text = fs::read_to_string(path(data_dir)).unwrap_or_default();
    let mut entries: Vec<Entry> = text
        .lines()
        .rev()
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(limit)
        .collect();
    entries.truncate(limit);
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_keeps_who_did_what_newest_first_without_the_mail_itself() {
        let dir = tempfile::tempdir().unwrap();
        assert!(recent(dir.path(), 5).is_empty());
        for (i, action) in ["mail.list", "mail.search", "mail.draft"]
            .iter()
            .enumerate()
        {
            record(
                dir.path(),
                &Entry {
                    at: i as i64,
                    agent: "fin".into(),
                    action: (*action).into(),
                    about: about(
                        action,
                        &serde_json::json!({"query": "from:bcp", "id": "a.inbox.1", "text": "secret reply"}),
                    ),
                    ok: true,
                },
            );
        }
        let all = recent(dir.path(), 5);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].action, "mail.draft");
        assert_eq!(all[1].about, "search «from:bcp»");
        let file = fs::read_to_string(dir.path().join("mail/activity.jsonl")).unwrap();
        assert!(!file.contains("secret reply"));
        assert_eq!(recent(dir.path(), 1).len(), 1);
    }
}
