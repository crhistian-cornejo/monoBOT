//! Optional mail infrastructure. Settings never contain credentials. Cloud mail
//! is pulled from a durable backlog, so no public desktop port is needed.
mod calendar;
mod feed;
mod inbound;
mod mcp;
mod secrets;
pub use calendar::{CalendarEvent, NewEvent};
pub use feed::{feed_url, FeedState, STALE_MS as FEED_STALE_MS};
pub use inbound::is_cloud;
pub use inbound::Delivery;
pub use mcp::run_mcp_proxy;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE: Mutex<()> = Mutex::new(());
const MAX_FILE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub mailflare_url: String,
    pub inbound_url: String,
    pub send_provider: String,
    pub cloudflare_account_id: String,
    pub routes: Vec<Route>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Route {
    pub alias: String,
    pub bot_id: String,
    pub allowed_senders: Vec<String>,
    pub enabled: bool,
}

#[derive(Clone)]
pub struct Service {
    pub data_dir: PathBuf,
}

impl Service {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.data_dir.join("mail").join(name)
    }
    pub fn config(&self) -> Result<Config, String> {
        read_json(&self.path("workspace.json"))
    }
    pub fn save_config(&self, config: &Config) -> Result<(), String> {
        let mut config = config.clone();
        config.mailflare_url = endpoint(&config.mailflare_url, true)?;
        config.inbound_url = endpoint(&config.inbound_url, false)?;
        if !matches!(
            config.send_provider.as_str(),
            "" | "smtp" | "resend" | "cloudflare"
        ) {
            return Err("Choose SMTP, Resend or Cloudflare.".into());
        }
        if config.send_provider == "cloudflare"
            && (config.cloudflare_account_id.len() != 32
                || !config
                    .cloudflare_account_id
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit()))
        {
            return Err("Enter the 32-character Cloudflare account ID.".into());
        }
        if config.routes.len() > 50 {
            return Err("At most 50 bot inboxes can be configured.".into());
        }
        let mut aliases = std::collections::HashSet::new();
        for route in &mut config.routes {
            route.alias = route.alias.trim().to_lowercase();
            if !monobot_email::valid_address(&route.alias)
                || !aliases.insert(route.alias.clone())
                || route.bot_id.is_empty()
                || route.bot_id.len() > 128
                || route.bot_id.chars().any(char::is_control)
                || route.allowed_senders.len() > 50
            {
                return Err("Each bot inbox needs a unique email alias and a bot ID.".into());
            }
            for sender in &mut route.allowed_senders {
                *sender = sender.trim().to_lowercase();
                if !monobot_email::valid_address(sender) {
                    return Err("Bot sender allowlists require complete email addresses.".into());
                }
            }
            if route.enabled && route.allowed_senders.is_empty() {
                return Err("Choose allowed senders before enabling a bot inbox.".into());
            }
        }
        let _guard = WRITE.lock().unwrap_or_else(|p| p.into_inner());
        write_json(&self.path("workspace.json"), &config)
    }
    pub fn set_secret(&self, kind: &str, value: &str) -> Result<(), String> {
        if !matches!(kind, "mailflare" | "resend" | "cloudflare" | "inbound") {
            return Err("Unknown mail credential.".into());
        }
        if !value.is_empty() {
            monobot_email::http_send::validate_token(value)?;
            if kind == "inbound" && value.len() < 32 {
                return Err("Inbound token must contain at least 32 characters.".into());
            }
        }
        secrets::set(&self.data_dir, kind, value)
    }
    pub(crate) fn secret(&self, kind: &str) -> Result<String, String> {
        secrets::get(&self.data_dir, kind)
    }
    pub(crate) fn outgoing(&self) -> Result<(monobot_email::http_send::Transport, String), String> {
        use monobot_email::http_send::Transport;
        let config = self.config()?;
        match config.send_provider.as_str() {
            "" | "smtp" => Ok((Transport::Smtp, String::new())),
            "resend" => Ok((Transport::Resend, self.secret("resend")?)),
            "cloudflare" => Ok((
                Transport::Cloudflare {
                    account_id: config.cloudflare_account_id,
                },
                self.secret("cloudflare")?,
            )),
            _ => Err("Invalid mail transport setting.".into()),
        }
    }
}

/// A remote service must be HTTPS. No credentials, query tokens or fragments.
pub(crate) fn endpoint(value: &str, mcp: bool) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    let mut url = url::Url::parse(value).map_err(|_| "Enter an HTTPS service URL.")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path().trim_end_matches('/'), "" | "/mcp")
        || (!mcp && url.path().trim_end_matches('/') == "/mcp")
    {
        return Err("Use an HTTPS origin without credentials, query or fragment.".into());
    }
    url.set_path(if mcp { "/mcp" } else { "" });
    Ok(url.to_string().trim_end_matches('/').to_owned())
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(T::default()),
        Err(_) => return Err("Could not read mail workspace data.".into()),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Could not read mail workspace data.")?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("Mail workspace data exceeds its size limit.".into());
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| "Mail workspace data is invalid; it was not overwritten.".into())
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let parent = path.parent().ok_or("Invalid workspace path.")?;
    std::fs::create_dir_all(parent).map_err(|_| "Could not create mail workspace folder.")?;
    let mut file = tempfile::NamedTempFile::new_in(parent)
        .map_err(|_| "Could not save mail workspace data.")?;
    let bytes = serde_json::to_vec(value).map_err(|_| "Could not encode mail workspace data.")?;
    if bytes.len() as u64 > MAX_FILE {
        return Err("Mail workspace storage is full; export or remove older data first.".into());
    }
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "Could not save mail workspace data.")?;
    file.persist(path)
        .map_err(|_| "Could not replace mail workspace data.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_is_opt_in_validated_and_has_no_tokens() {
        let dir = tempfile::tempdir().unwrap();
        let service = Service::new(dir.path());
        assert!(service.config().unwrap().routes.is_empty());
        let mut config = Config {
            mailflare_url: "https://mail.example.com".into(),
            ..Config::default()
        };
        service.save_config(&config).unwrap();
        assert_eq!(
            service.config().unwrap().mailflare_url,
            "https://mail.example.com/mcp"
        );
        config.routes.push(Route {
            alias: "bot@example.com".into(),
            bot_id: "bot".into(),
            enabled: true,
            ..Route::default()
        });
        assert!(service.save_config(&config).is_err());
        for url in [
            "http://example.com",
            "https://secret@example.com",
            "https://example.com?token=secret",
            "https://example.com/#secret",
        ] {
            assert!(endpoint(url, true).is_err());
        }
    }
}
