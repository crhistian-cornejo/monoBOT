//! API keys for the live voice providers, in the system keychain (service
//! `com.monocode.desktop`, account `voice-<provider>`), as IVY keeps its own.
//! Settings only ever learns whether a key is saved. Tests use an in-memory
//! store instead of the keychain (`keep_in_memory` for the app's).
use std::fmt;

/// A cloud provider a call can talk through in real time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Provider {
    /// Gemini Live (`BidiGenerateContent`), with a Google AI Studio key.
    Gemini,
    /// Phonic speech-to-speech, with a Phonic key.
    Phonic,
}

impl Provider {
    pub const ALL: [Self; 2] = [Self::Gemini, Self::Phonic];

    pub fn id(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Phonic => "phonic",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|provider| provider.id() == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gemini => "Gemini Live",
            Self::Phonic => "Phonic",
        }
    }

    /// Where the user gets a key.
    pub fn key_page(self) -> &'static str {
        match self {
            Self::Gemini => "https://aistudio.google.com/apikey",
            Self::Phonic => "https://phonic.co",
        }
    }

    fn account(self) -> String {
        format!("voice-{}", self.id())
    }
}

/// A key, never printed: `Debug` and `Display` say only that it is there.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiKey(String);

impl ApiKey {
    /// Checks what the user pasted: trimmed, 16 to 200 printable ASCII
    /// characters with no spaces inside. Anything else is a paste mistake,
    /// not a key.
    pub fn parse(value: &str) -> Result<Self, KeyError> {
        let value = value.trim();
        if value.is_empty() {
            return Err(KeyError::Empty);
        }
        if !(16..=200).contains(&value.len()) {
            return Err(KeyError::Length);
        }
        if !value.chars().all(|c| c.is_ascii_graphic()) {
            return Err(KeyError::Characters);
        }
        Ok(Self(value.to_owned()))
    }

    /// The key itself, for the request header that needs it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(…)")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyError {
    Empty,
    Length,
    Characters,
    Keychain,
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "Paste the API key first.",
            Self::Length => {
                "That does not look like an API key: it should be 16 to 200 characters."
            }
            Self::Characters => {
                "That does not look like an API key: it has spaces or unusual characters."
            }
            Self::Keychain => "The system keychain is unavailable.",
        })
    }
}

impl std::error::Error for KeyError {}

/// Saves `key` for `provider`, replacing any saved before.
pub fn save(provider: Provider, key: &ApiKey) -> Result<(), KeyError> {
    store::set(&provider.account(), Some(key.expose()))
}

/// Forgets the key for `provider`; nothing saved is not an error.
pub fn remove(provider: Provider) -> Result<(), KeyError> {
    store::set(&provider.account(), None)
}

/// The saved key, if any.
pub fn get(provider: Provider) -> Option<ApiKey> {
    store::get(&provider.account()).and_then(|value| ApiKey::parse(&value).ok())
}

/// Whether a key is saved, without handing it out.
pub fn saved(provider: Provider) -> bool {
    get(provider).is_some()
}

/// Another secret of the app's under the same keychain service (a sign-in's
/// refresh token), by its own account name.
pub(crate) fn secret(account: &str) -> Option<String> {
    store::get(account)
}

/// Saves or, with `None`, forgets such a secret.
pub(crate) fn set_secret(account: &str, value: Option<&str>) -> Result<(), KeyError> {
    store::set(account, value)
}

/// Keeps keys in this process's memory instead of the keychain, for the
/// app's own tests: nothing they save reaches the real keychain.
pub fn keep_in_memory() {
    store::MEMORY_ONLY.store(true, std::sync::atomic::Ordering::Release);
}

mod store {
    use super::KeyError;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};

    const SERVICE: &str = "com.monocode.desktop";
    pub(super) static MEMORY_ONLY: AtomicBool = AtomicBool::new(cfg!(test));

    fn memory() -> &'static Mutex<HashMap<String, String>> {
        static MEMORY: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
        MEMORY.get_or_init(Default::default)
    }

    fn in_memory() -> bool {
        MEMORY_ONLY.load(Ordering::Acquire)
    }

    pub(super) fn set(account: &str, value: Option<&str>) -> Result<(), KeyError> {
        if in_memory() {
            let mut memory = memory().lock().unwrap_or_else(|e| e.into_inner());
            match value {
                Some(value) => memory.insert(account.into(), value.into()),
                None => memory.remove(account),
            };
            return Ok(());
        }
        let entry = keyring::Entry::new(SERVICE, account).map_err(|_| KeyError::Keychain)?;
        match value {
            Some(value) => entry.set_password(value).map_err(|_| KeyError::Keychain),
            None => match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err(KeyError::Keychain),
            },
        }
    }

    pub(super) fn get(account: &str) -> Option<String> {
        if in_memory() {
            return memory()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(account)
                .cloned();
        }
        keyring::Entry::new(SERVICE, account)
            .and_then(|entry| entry.get_password())
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "AIzaSyD-example-key-0123456789";

    #[test]
    fn a_pasted_key_is_trimmed_and_checked() {
        assert_eq!(
            ApiKey::parse(&format!("  {GOOD}\n")).unwrap().expose(),
            GOOD
        );
        assert_eq!(ApiKey::parse("   "), Err(KeyError::Empty));
        assert_eq!(ApiKey::parse("short"), Err(KeyError::Length));
        assert_eq!(ApiKey::parse(&"x".repeat(201)), Err(KeyError::Length));
        assert_eq!(
            ApiKey::parse("AIzaSy with a space inside"),
            Err(KeyError::Characters)
        );
        assert_eq!(
            ApiKey::parse("AIzaSyñ-example-key-0123"),
            Err(KeyError::Characters)
        );
    }

    #[test]
    fn a_key_never_prints() {
        let key = ApiKey::parse(GOOD).unwrap();
        assert_eq!(format!("{key:?}"), "ApiKey(…)");
        assert!(!format!("{:?}", Some(key)).contains("AIza"));
    }

    #[test]
    fn keys_are_saved_per_provider_and_removed() {
        let key = ApiKey::parse(GOOD).unwrap();
        remove(Provider::Gemini).unwrap();
        remove(Provider::Phonic).unwrap();
        assert!(!saved(Provider::Gemini));
        save(Provider::Gemini, &key).unwrap();
        assert!(saved(Provider::Gemini));
        assert!(!saved(Provider::Phonic), "each provider has its own");
        assert_eq!(get(Provider::Gemini), Some(key));
        remove(Provider::Gemini).unwrap();
        assert_eq!(get(Provider::Gemini), None);
        // Removing what is not there is fine.
        remove(Provider::Gemini).unwrap();
    }

    #[test]
    fn providers_round_trip_their_ids() {
        for provider in Provider::ALL {
            assert_eq!(Provider::from_id(provider.id()), Some(provider));
            assert!(provider.key_page().starts_with("https://"));
            assert_eq!(provider.account(), format!("voice-{}", provider.id()));
        }
        assert_eq!(Provider::from_id("openai"), None);
    }
}
