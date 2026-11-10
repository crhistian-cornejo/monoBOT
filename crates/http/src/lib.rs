//! Who monoBOT trusts when it opens an HTTPS connection of its own (model
//! downloads, Azure DevOps, GitLab, Jira, Linear, link previews…).
//!
//! On the Mac, the web's public authorities bundled with the app, as always.
//! On Windows, the system checks each certificate against its own store: a
//! company network that inspects its traffic answers with certificates signed
//! by an authority its administrators put there, and with the bundled list
//! alone every connection fails with "UnknownIssuer". Nothing is switched off:
//! a certificate nobody trusts is still refused.

/// A `ureq` agent builder with the app's certificate checks. Add timeouts,
/// redirects and the like as before, then `build()`.
pub fn agent() -> ureq::AgentBuilder {
    let builder = ureq::AgentBuilder::new();
    #[cfg(windows)]
    let builder = match windows::config() {
        Ok(config) => builder.tls_config(config),
        // The bundled list still works on any network that is not inspected.
        Err(_) => builder,
    };
    builder
}

#[cfg(windows)]
mod windows {
    use std::sync::{Arc, OnceLock};

    /// Built once: the verifier loads the system store when it is made.
    pub fn config() -> Result<Arc<rustls::ClientConfig>, String> {
        static CONFIG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
        CONFIG.get_or_init(build).clone()
    }

    fn build() -> Result<Arc<rustls::ClientConfig>, String> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|error| error.to_string())?;
        let config = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|error| error.to_string())?
            // Not dangerous: the verifier is Windows' own certificate check.
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        Ok(Arc::new(config))
    }
}

#[cfg(test)]
mod tests {
    /// Should the system's verifier fail to load, `agent()` quietly keeps the
    /// bundled list, and every inspected network fails again.
    #[cfg(windows)]
    #[test]
    fn windows_checks_certificates_with_its_own_store() {
        super::windows::config().unwrap();
    }

    /// Reaches GitHub with the app's client; needs the network, so run it by
    /// hand: `cargo test -p monobot-http -- --ignored`.
    #[test]
    #[ignore]
    fn reaches_github() {
        let response = super::agent()
            .build()
            .head("https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx")
            .call()
            .unwrap();
        assert!(response.status() < 400);
    }
}
