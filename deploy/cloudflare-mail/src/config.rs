//! Worker settings: recipient allowlist, envelope addresses, token and retention.

use std::collections::HashSet;

pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MIN_TOKEN_LENGTH: usize = 32;
pub const DEFAULT_RETENTION_DAYS: i64 = 30;
pub const MAX_RETENTION_DAYS: i64 = 365;
pub const DEFAULT_AUTHSERV_ID: &str = "mx.cloudflare.net";
pub const R2_KEY_PREFIX: &str = "mail/";

/// The Worker's variables and secret, as Wrangler binds them.
#[derive(Clone, Debug, Default)]
pub struct Vars {
    /// Secret. Must be at least 32 characters or the HTTP API refuses to serve.
    pub inbound_token: Option<String>,
    /// Required. Comma separated exact addresses or `*@domain`.
    pub allowed_recipients: Option<String>,
    pub retention_days: Option<String>,
    pub trusted_authserv_id: Option<String>,
}

impl Vars {
    /// The token, only when it is long enough to serve with.
    pub fn valid_token(&self) -> Option<&str> {
        self.inbound_token
            .as_deref()
            .filter(|token| token.len() >= MIN_TOKEN_LENGTH)
    }

    pub fn authserv_id(&self) -> &str {
        self.trusted_authserv_id
            .as_deref()
            .unwrap_or(DEFAULT_AUTHSERV_ID)
    }
}

#[derive(Debug)]
pub struct RecipientAllowlist {
    addresses: HashSet<String>,
    domains: HashSet<String>,
}

/// Parses `ALLOWED_RECIPIENTS`. Empty or any invalid entry gives `None`, so
/// the email handler fails closed.
pub fn parse_allowlist(raw: Option<&str>) -> Option<RecipientAllowlist> {
    let mut addresses = HashSet::new();
    let mut domains = HashSet::new();
    for entry in raw.unwrap_or("").split(',') {
        let value = entry.trim().to_lowercase();
        if value.is_empty() {
            continue;
        }
        if let Some(domain) = value.strip_prefix("*@") {
            if !is_domain(domain) {
                return None;
            }
            domains.insert(domain.to_string());
        } else {
            if !is_address(&value) {
                return None;
            }
            addresses.insert(value);
        }
    }
    if addresses.is_empty() && domains.is_empty() {
        return None;
    }
    Some(RecipientAllowlist { addresses, domains })
}

impl RecipientAllowlist {
    /// Exact address, or exact domain of a `*@domain` entry (never a subdomain).
    pub fn allows(&self, address: &str) -> bool {
        if self.addresses.contains(address) {
            return true;
        }
        match address.rfind('@') {
            Some(at) if at > 0 => self.domains.contains(&address[at + 1..]),
            _ => false,
        }
    }
}

fn is_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    (1..=63).contains(&bytes.len())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes[0] != b'-'
        && bytes[bytes.len() - 1] != b'-'
}

/// A lowercase host name with at least two labels.
fn is_domain(value: &str) -> bool {
    (1..=253).contains(&value.len())
        && value.split('.').count() >= 2
        && value.split('.').all(is_label)
}

fn is_local_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || "!#$%&'*+/=?^_`{|}~.-".contains(c)
}

fn is_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    (1..=64).contains(&local.len())
        && local.chars().all(is_local_char)
        && !domain.is_empty()
        && !domain.contains('@')
        && !domain.chars().any(char::is_whitespace)
        && is_domain(domain)
}

/// Lowercased envelope address, or `None` when it is not a plain mailbox.
pub fn normalize_envelope_address(value: &str) -> Option<String> {
    let address = value.trim().to_lowercase();
    (address.len() <= 254 && is_address(&address)).then_some(address)
}

/// `RETENTION_DAYS` as 1..=365, falling back to 30 for anything else.
pub fn retention_days(raw: Option<&str>) -> i64 {
    let Some(raw) = raw.map(str::trim) else {
        return DEFAULT_RETENTION_DAYS;
    };
    if raw.is_empty() || raw.len() > 3 || !raw.bytes().all(|b| b.is_ascii_digit()) {
        return DEFAULT_RETENTION_DAYS;
    }
    match raw.parse::<i64>() {
        Ok(days) if (1..=MAX_RETENTION_DAYS).contains(&days) => days,
        _ => DEFAULT_RETENTION_DAYS,
    }
}
