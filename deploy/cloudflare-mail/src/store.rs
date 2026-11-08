//! The two storage surfaces the Worker uses: the D1 index and the R2 bodies.
//! Cloudflare's bindings implement them in `cf.rs`; tests use SQLite and a map.

use std::fmt;

/// A storage failure. It carries no message, so nothing private can reach a log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageError;

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("storage unavailable")
    }
}

impl std::error::Error for StorageError {}

pub type Result<T> = std::result::Result<T, StorageError>;

/// One indexed message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub seq: i64,
    pub envelope_from: String,
    pub envelope_to: String,
    pub received_at: i64,
    pub size: i64,
    pub auth_failed: bool,
}

pub struct NewRow<'a> {
    pub r2_key: &'a str,
    pub envelope_from: &'a str,
    pub envelope_to: &'a str,
    pub received_at: i64,
    pub size: i64,
    pub auth_failed: bool,
}

/// The `messages` table (`migrations/0001_init.sql`).
#[allow(async_fn_in_trait)]
pub trait Index {
    async fn insert(&self, row: NewRow<'_>) -> Result<()>;
    /// Rows with `seq > after`, oldest first.
    async fn after(&self, after: i64, limit: i64) -> Result<Vec<Row>>;
    async fn key_of(&self, seq: i64) -> Result<Option<String>>;
    /// `(seq, r2_key)` of rows received before `cutoff`, oldest first.
    async fn expired(&self, cutoff: i64, limit: i64) -> Result<Vec<(i64, String)>>;
    async fn delete(&self, seqs: &[i64]) -> Result<()>;
}

/// Raw MIME bodies by key.
#[allow(async_fn_in_trait)]
pub trait Bodies {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn delete(&self, keys: &[String]) -> Result<()>;
}

pub const INSERT_SQL: &str = "INSERT INTO messages (r2_key, envelope_from, envelope_to, received_at, size, auth_failed) VALUES (?, ?, ?, ?, ?, ?)";
pub const AFTER_SQL: &str = "SELECT seq, envelope_from, envelope_to, received_at, size, auth_failed FROM messages WHERE seq > ? ORDER BY seq ASC LIMIT ?";
pub const KEY_SQL: &str = "SELECT r2_key FROM messages WHERE seq = ?";
pub const EXPIRED_SQL: &str =
    "SELECT seq, r2_key FROM messages WHERE received_at < ? ORDER BY seq ASC LIMIT ?";

pub fn delete_sql(count: usize) -> String {
    format!(
        "DELETE FROM messages WHERE seq IN ({})",
        vec!["?"; count].join(",")
    )
}
