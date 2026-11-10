//! What Echo's `echo-base` gave this crate, made from monoBOT's own pieces: the
//! system's store of secrets, a log line, and the TLS settings of a connection
//! opened by hand (IMAP and SMTP).

pub mod keychain {
    /// The Keychain on macOS, Credential Manager on Windows. Tests use a store
    /// in memory, so they never read, or make the system ask for, real secrets.
    #[cfg_attr(test, allow(dead_code))]
    pub fn entry(service: &str, name: &str) -> keyring::Result<keyring::Entry> {
        #[cfg(test)]
        {
            static IN_MEMORY: std::sync::Once = std::sync::Once::new();
            IN_MEMORY.call_once(|| {
                keyring::set_default_credential_builder(keyring::mock::default_credential_builder())
            });
        }
        keyring::Entry::new(service, name)
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
