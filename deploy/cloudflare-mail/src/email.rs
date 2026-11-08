//! Receiving one message from Cloudflare Email Routing.

use crate::auth_results::is_auth_failed;
use crate::config::{normalize_envelope_address, parse_allowlist, Vars, MAX_MESSAGE_BYTES};
use crate::log;
use crate::store::{Bodies, Index, NewRow, StorageError};
use std::fmt;

/// The message as the Email Worker runtime hands it over.
#[allow(async_fn_in_trait)]
pub trait Inbound {
    /// SMTP envelope `MAIL FROM`.
    fn from(&self) -> String;
    /// SMTP envelope `RCPT TO`.
    fn to(&self) -> String;
    fn raw_size(&self) -> f64;
    fn set_reject(&self, reason: &str);
    /// The raw MIME, or `None` (stream cancelled) once it exceeds `max` bytes.
    async fn read_capped(&self, max: usize) -> Result<Option<Vec<u8>>, StorageError>;
}

/// Why a message could not be accepted. Returning it makes Cloudflare report
/// a temporary failure instead of accepting the message.
#[derive(Debug, PartialEq, Eq)]
pub enum EmailError {
    NotConfigured,
    Storage,
}

impl fmt::Display for EmailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            EmailError::NotConfigured => "ALLOWED_RECIPIENTS is not configured",
            EmailError::Storage => "storage unavailable",
        })
    }
}

impl std::error::Error for EmailError {}

impl From<StorageError> for EmailError {
    fn from(_: StorageError) -> Self {
        EmailError::Storage
    }
}

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// Persists one inbound message: raw MIME to R2 first, then the D1 index row.
///
/// Permanent refusals use `set_reject`. Infrastructure failures return an
/// error so Cloudflare does not report the message as accepted; an index row
/// is only ever written after its body is durable, so the pull API never lists
/// a message without a body. A crash between the two writes leaves an
/// unindexed R2 object, which the bucket lifecycle rule removes.
pub async fn handle(
    message: &impl Inbound,
    vars: &Vars,
    index: &impl Index,
    bodies: &impl Bodies,
    now: i64,
    key: String,
) -> Result<(), EmailError> {
    let Some(allowlist) = parse_allowlist(vars.allowed_recipients.as_deref()) else {
        log::error("inbound: ALLOWED_RECIPIENTS is missing or invalid");
        return Err(EmailError::NotConfigured);
    };
    let Some(to) = normalize_envelope_address(&message.to()).filter(|to| allowlist.allows(to))
    else {
        message.set_reject("550 5.1.1 Unknown recipient");
        return Ok(());
    };
    let Some(from) = normalize_envelope_address(&message.from()) else {
        message.set_reject("550 5.7.1 Sender not accepted");
        return Ok(());
    };
    let size = message.raw_size();
    let safe = size.is_finite() && size.fract() == 0.0 && size.abs() <= MAX_SAFE_INTEGER;
    if !safe || size > MAX_MESSAGE_BYTES as f64 {
        message.set_reject("552 5.3.4 Message too large");
        return Ok(());
    }
    let Some(raw) = message.read_capped(MAX_MESSAGE_BYTES).await? else {
        message.set_reject("552 5.3.4 Message too large");
        return Ok(());
    };

    let auth_failed = is_auth_failed(&raw, &from, vars.authserv_id());
    bodies.put(&key, &raw).await?;
    let row = NewRow {
        r2_key: &key,
        envelope_from: &from,
        envelope_to: &to,
        received_at: now,
        size: raw.len() as i64,
        auth_failed,
    };
    if let Err(error) = index.insert(row).await {
        let _ = bodies.delete(std::slice::from_ref(&key)).await;
        return Err(error.into());
    }
    Ok(())
}
