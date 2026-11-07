//! What one agent asked another with agents.message: the Principal handing a
//! bot its part, mostly (delegations.ts). The Principal's Bots panel lists
//! them by bot, and agents.list shows each agent what it was asked lately.
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub id: String,
    /// The Mono that asked.
    pub from: String,
    /// The Mono it asked.
    pub to: String,
    pub message: String,
    pub at: u64,
    /// What it answered, when the asker waited for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
}

/// The desktop's key; the native store keeps its own copy under it, seeded
/// from the desktop the first time.
pub const KEY: &str = "monocode.delegations.v1";
/// Enough for weeks of work; older ones drop off.
pub const MAX: usize = 200;
const REPLY_MAX: usize = 4_000;

/// Entries with every field the desktop requires; anything else is dropped.
pub fn parse(text: &str) -> Vec<Delegation> {
    serde_json::from_str::<Vec<serde_json::Value>>(text)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|value| serde_json::from_value(value).ok())
        .collect()
}

/// Newest first.
pub fn load(store: &Store) -> Vec<Delegation> {
    if let Ok(Some(text)) = store.setting(KEY) {
        return parse(&text);
    }
    store
        .desktop_preferences()
        .and_then(|preferences| preferences.get(KEY))
        .map(parse)
        .unwrap_or_default()
}

pub fn save(store: &Store, list: &[Delegation]) -> Result<(), String> {
    store.save_setting(
        KEY,
        &serde_json::to_string(list).map_err(|e| e.to_string())?,
    )
}

/// Records a request at the front and returns its id.
pub fn record(list: &mut Vec<Delegation>, from: &str, to: &str, message: &str, at: u64) -> String {
    let id = super::new_id();
    list.insert(
        0,
        Delegation {
            id: id.clone(),
            from: from.into(),
            to: to.into(),
            message: message.into(),
            at,
            reply: None,
        },
    );
    list.truncate(MAX);
    id
}

/// Keeps the answer an agent waited for beside what it asked.
pub fn answer(list: &mut [Delegation], id: &str, reply: &str) -> bool {
    let Some(entry) = list.iter_mut().find(|entry| entry.id == id) else {
        return false;
    };
    entry.reply = Some(reply.chars().take(REPLY_MAX).collect());
    true
}

/// The latest request to each agent from `from`, newest first: one per agent.
pub fn latest_by_agent<'a>(list: &'a [Delegation], from: &str) -> Vec<&'a Delegation> {
    let mut seen = std::collections::HashSet::new();
    list.iter()
        .filter(|entry| entry.from == from && seen.insert(entry.to.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_newest_first_and_keeps_waited_answers() {
        let mut list = Vec::new();
        let first = record(&mut list, "p", "a", "find the invoice", 1);
        record(&mut list, "p", "b", "sum it", 2);
        record(&mut list, "p", "a", "again", 3);
        assert_eq!(list[0].message, "again");
        assert!(answer(&mut list, &first, &"x".repeat(5_000)));
        assert_eq!(list[2].reply.as_ref().unwrap().len(), 4_000);
        assert!(!answer(&mut list, "missing", "y"));
        let latest: Vec<_> = latest_by_agent(&list, "p")
            .iter()
            .map(|d| d.message.as_str())
            .collect();
        assert_eq!(latest, ["again", "sum it"]);
        for at in 0..300 {
            record(&mut list, "p", "a", "m", at);
        }
        assert_eq!(list.len(), MAX);
    }

    #[test]
    fn parse_drops_incomplete_entries() {
        let text = r#"[{"id":"1","from":"a","to":"b","message":"m","at":5},{"id":"2","from":"a"}]"#;
        let list = parse(text);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].at, 5);
        assert!(parse("not json").is_empty());
    }
}
