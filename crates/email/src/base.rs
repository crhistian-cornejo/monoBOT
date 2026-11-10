//! What Echo's `echo-base` gave this crate, made from monoBOT's own pieces: the
//! system's store of secrets, a log line, and the TLS settings of a connection
//! opened by hand (IMAP and SMTP).

/// Where every secret of the app is kept: the Keychain on macOS, Credential
/// Manager on Windows, the secret service on Linux.
///
/// Only after the app says so. Until [`use_system`] is called, which the
/// app's `main` does first thing, secrets live in this process's memory. So
/// no test, of this crate or of one that links it, ever reads the user's own
/// secrets or makes the system ask for them: a test binary never calls it.
pub mod keychain {
    use std::any::Any;
    use std::collections::HashMap;
    #[cfg(target_os = "macos")]
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static SYSTEM: AtomicBool = AtomicBool::new(false);

    /// From now on secrets are the system's to keep. Called once, by the app
    /// itself, before it reads any.
    pub fn use_system() {
        SYSTEM.store(true, Ordering::Release);
    }

    /// Whether the system's store is in use: false in every test.
    pub fn uses_system() -> bool {
        SYSTEM.load(Ordering::Acquire)
    }

    /// The secret `name` of `service`, wherever secrets are kept now.
    pub fn entry(service: &str, name: &str) -> keyring::Result<keyring::Entry> {
        if !SYSTEM.load(Ordering::Acquire) {
            return Ok(keyring::Entry::new_with_credential(Box::new(InMemory(
                format!("{service}\n{name}"),
            ))));
        }
        name_item(service, name);
        keyring::Entry::new(service, name)
    }

    /// A secret in this process's memory, found again by any entry made for
    /// the same service and name, as the system's store would.
    struct InMemory(String);

    fn memory() -> MutexGuard<'static, HashMap<String, Vec<u8>>> {
        static MEMORY: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
        MEMORY
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    impl keyring::credential::CredentialApi for InMemory {
        fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
            memory().insert(self.0.clone(), secret.to_vec());
            Ok(())
        }
        fn get_secret(&self) -> keyring::Result<Vec<u8>> {
            memory()
                .get(&self.0)
                .cloned()
                .ok_or(keyring::Error::NoEntry)
        }
        fn delete_credential(&self) -> keyring::Result<()> {
            memory()
                .remove(&self.0)
                .map(|_| ())
                .ok_or(keyring::Error::NoEntry)
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// What the Keychain's own dialog calls the app's secrets.
    #[cfg(target_os = "macos")]
    const ITEM_NAME: &str = "monoBOT";
    /// On an item this module made under its name: its dialog reads right.
    #[cfg(target_os = "macos")]
    const ITEM_NOTE: &str = "monoBOT's own; made under its name";

    /// Makes sure the Keychain's dialog names the app. macOS words it from
    /// the access list an item was made with ("monoBOT wants to use … stored
    /// in “com.monocode.desktop”"), and a store that names nothing leaves the
    /// service's internal id there. Giving the item a name afterwards changes
    /// what Keychain Access shows and not what the dialog says, so an item
    /// not made under the name is made again ([`remake`]). The service and
    /// the account, which find the secret, stay as they are. Once per item
    /// and run; an item that does not exist yet is seen to the next time.
    #[cfg(target_os = "macos")]
    fn name_item(service: &str, account: &str) {
        use security_framework::item::{ItemClass, ItemSearchOptions};
        static NAMED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let key = format!("{service}\n{account}");
        let named = NAMED.get_or_init(Default::default);
        if named
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&key)
        {
            return;
        }
        // Reading an item's attributes asks the user nothing.
        let mut search = ItemSearchOptions::new();
        search
            .class(ItemClass::generic_password())
            .service(service)
            .account(account)
            .load_attributes(true);
        let Ok(found) = search.search() else {
            return;
        };
        let Some(attributes) = found.iter().find_map(|result| result.simplify_dict()) else {
            return;
        };
        let ours = |attribute: &str, value: &str| {
            attributes.get(attribute).map(String::as_str) == Some(value)
        };
        if !(ours("labl", ITEM_NAME) && ours("icmt", ITEM_NOTE)) && !remake(service, account) {
            return;
        }
        named
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(key);
    }

    /// Makes the item again under the app's name, with the same secret. The
    /// secret is read first, which asks the user exactly as the read that
    /// follows was about to, and for the last time under the old name. If
    /// the named item cannot be added, the secret goes back as it was rather
    /// than be lost. False when nothing was changed.
    #[cfg(target_os = "macos")]
    fn remake(service: &str, account: &str) -> bool {
        use security_framework::passwords::{
            PasswordOptions, delete_generic_password, get_generic_password, set_generic_password,
            set_generic_password_options,
        };
        let Ok(secret) = get_generic_password(service, account) else {
            return false;
        };
        if delete_generic_password(service, account).is_err() {
            return false;
        }
        let mut named = PasswordOptions::new_generic_password(service, account);
        named.set_label(ITEM_NAME);
        named.set_comment(ITEM_NOTE);
        if set_generic_password_options(&secret, named).is_ok() {
            return true;
        }
        let _ = set_generic_password(service, account, &secret);
        false
    }

    #[cfg(not(target_os = "macos"))]
    fn name_item(_: &str, _: &str) {}

    #[cfg(test)]
    mod tests {
        #[test]
        fn until_the_app_says_so_secrets_live_in_memory_and_are_found_again() {
            let saved = super::entry("com.monocode.desktop", "test-secret").unwrap();
            assert!(matches!(saved.get_password(), Err(keyring::Error::NoEntry)));
            saved.set_password("abc").unwrap();
            // Another entry for the same secret finds it, as the system's
            // store would; another name does not.
            let again = super::entry("com.monocode.desktop", "test-secret").unwrap();
            assert_eq!(again.get_password().unwrap(), "abc");
            let other = super::entry("com.monocode.desktop", "another").unwrap();
            assert!(matches!(other.get_password(), Err(keyring::Error::NoEntry)));
            again.delete_credential().unwrap();
            assert!(matches!(saved.get_password(), Err(keyring::Error::NoEntry)));
            assert!(matches!(
                saved.delete_credential(),
                Err(keyring::Error::NoEntry)
            ));
        }
    }
}

pub mod log {
    pub fn line(message: impl AsRef<str>) {
        eprintln!("[mail] {}", message.as_ref());
    }
}

pub mod tls {
    use std::sync::Arc;

    /// Who is trusted on a mail connection: on Windows the system's own store
    /// (a company network that inspects traffic signs with an authority only
    /// that store has), elsewhere the web's public authorities bundled with the
    /// app. A certificate nobody trusts is still refused.
    pub fn client() -> Result<rustls::ClientConfig, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS: {e}"))?;
        #[cfg(windows)]
        {
            let verifier = rustls_platform_verifier::Verifier::new(provider)
                .map_err(|e| format!("TLS: {e}"))?;
            // Not dangerous: the verifier is the system's own certificate check.
            Ok(builder
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier))
                .with_no_client_auth())
        }
        #[cfg(not(windows))]
        {
            let _ = provider;
            let roots = rustls::RootCertStore {
                roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
            };
            Ok(builder.with_root_certificates(roots).with_no_client_auth())
        }
    }
}
