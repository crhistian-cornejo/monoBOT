//! Context that a provider keeps for the rest of its conversation is sent
//! once, not with every message. Every message the provider already holds is
//! paid for again on each later request, so standing rules repeated per turn
//! fill the context and the bill for nothing; DeepSeek Harness keeps them out
//! of the repeated part for the same reason. A receipt in the store records
//! which provider conversation already has the context. Only a completed turn
//! writes it, and a compaction, which may summarize the context away, clears
//! it so the next turn sends it again.
use crate::store::Store;
use serde_json::Value;

pub struct Introduction {
    key: String,
    about: Value,
}

impl Introduction {
    /// `key` names the conversation and what is introduced in it; `about`
    /// describes the context itself, so a change sends it again. The receipt
    /// is `about` with the provider's `providerId`, in place of a
    /// `"providerId"` already in `about` or after its fields.
    pub fn new(key: impl Into<String>, about: Value) -> Self {
        Self {
            key: key.into(),
            about,
        }
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    /// Whether the provider conversation `provider_id` still needs it. A
    /// conversation the provider has not named yet always does.
    pub fn due(&self, store: &Store, provider_id: Option<&str>) -> Result<bool, String> {
        let Some(provider_id) = provider_id else {
            return Ok(true);
        };
        Ok(store.setting(&self.key)?.as_deref() != Some(self.receipt(provider_id).as_str()))
    }

    /// Call only after the introduction reached a completed turn. A provider
    /// that has not supplied a conversation id stays due.
    pub fn remember(&self, store: &Store, provider_id: Option<&str>) -> Result<(), String> {
        match provider_id.filter(|id| !id.trim().is_empty()) {
            Some(provider_id) => store.save_setting(&self.key, &self.receipt(provider_id)),
            None => Ok(()),
        }
    }

    /// After a compaction: send it again next time.
    pub fn forget(&self, store: &Store) -> Result<(), String> {
        store.save_setting(&self.key, "")
    }

    pub fn receipt(&self, provider_id: &str) -> String {
        let mut receipt = self.about.clone();
        if let Some(fields) = receipt.as_object_mut() {
            fields.insert("providerId".into(), provider_id.into());
        }
        receipt.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> (Fixture, Store) {
        let path = std::env::temp_dir().join(format!(
            "monocode-introduction-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(&path).unwrap();
        let store = Store::open_monocode(&path.join("sessions.db")).unwrap();
        (Fixture(path), store)
    }

    #[test]
    fn context_goes_once_per_provider_conversation_and_again_after_a_change() {
        let (_fixture, store) = fixture();
        let rules = Introduction::new("operator:1:codex:default", json!({"rules":"a"}));
        assert!(rules.due(&store, None).unwrap(), "an unnamed conversation");
        assert!(rules.due(&store, Some("thread")).unwrap());
        rules.remember(&store, None).unwrap();
        assert!(
            rules.due(&store, Some("thread")).unwrap(),
            "no id, no receipt"
        );
        rules.remember(&store, Some("thread")).unwrap();
        assert!(!rules.due(&store, Some("thread")).unwrap());
        assert!(
            rules.due(&store, Some("other")).unwrap(),
            "a new conversation"
        );
        let changed = Introduction::new(rules.key(), json!({"rules":"b"}));
        assert!(changed.due(&store, Some("thread")).unwrap(), "new rules");
        rules.forget(&store).unwrap();
        assert!(
            rules.due(&store, Some("thread")).unwrap(),
            "after a compaction"
        );
    }
}
