//! Same coverage as the former TypeScript suite: the real migration on an
//! in-memory SQLite, a map for R2 and a fake Email Worker message.

use crate::config::{Vars, MAX_MESSAGE_BYTES};
use crate::store::{self, Bodies, Index, NewRow, Row, StorageError};
use crate::{email, http, log, retention};
use futures::executor::block_on;
use rusqlite::{params, params_from_iter, Connection};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use url::Url;

const TOKEN: &str = "tttttttttttttttttttttttttttttttttttttttt";
const BOT: &str = "bot@example.com";
const MIGRATION: &str = include_str!("../migrations/0001_init.sql");
const CF_PASS: &str = "Authentication-Results: mx.cloudflare.net; dkim=pass header.d=sender.test; dmarc=pass header.from=sender.test policy.dmarc=none; spf=pass (mx.cloudflare.net: domain of alice@sender.test designates 192.0.2.1 as permitted sender) smtp.mailfrom=alice@sender.test";
const DAY: i64 = 24 * 60 * 60 * 1000;

struct Db {
    conn: Connection,
    fail_insert: Cell<bool>,
    fail_reads: Cell<bool>,
    fail_deletes: Cell<bool>,
}

impl Db {
    fn new() -> Self {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(MIGRATION).unwrap();
        Db {
            conn,
            fail_insert: Cell::new(false),
            fail_reads: Cell::new(false),
            fail_deletes: Cell::new(false),
        }
    }

    fn rows(&self) -> Vec<(i64, String, String, String, i64, i64, i64)> {
        let mut statement = self.conn.prepare("SELECT seq, r2_key, envelope_from, envelope_to, received_at, size, auth_failed FROM messages ORDER BY seq").unwrap();
        statement
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn seqs(&self) -> Vec<i64> {
        self.rows().into_iter().map(|row| row.0).collect()
    }

    fn check(&self, fail: &Cell<bool>) -> store::Result<()> {
        if fail.get() {
            Err(StorageError)
        } else {
            Ok(())
        }
    }
}

impl Index for Db {
    async fn insert(&self, row: NewRow<'_>) -> store::Result<()> {
        if self.fail_insert.replace(false) {
            return Err(StorageError);
        }
        self.conn
            .execute(
                store::INSERT_SQL,
                params![
                    row.r2_key,
                    row.envelope_from,
                    row.envelope_to,
                    row.received_at,
                    row.size,
                    i64::from(row.auth_failed)
                ],
            )
            .map(|_| ())
            .map_err(|_| StorageError)
    }

    async fn after(&self, after: i64, limit: i64) -> store::Result<Vec<Row>> {
        self.check(&self.fail_reads)?;
        let mut statement = self.conn.prepare(store::AFTER_SQL).unwrap();
        Ok(statement
            .query_map(params![after, limit], |r| {
                Ok(Row {
                    seq: r.get(0)?,
                    envelope_from: r.get(1)?,
                    envelope_to: r.get(2)?,
                    received_at: r.get(3)?,
                    size: r.get(4)?,
                    auth_failed: r.get::<_, i64>(5)? != 0,
                })
            })
            .unwrap()
            .map(Result::unwrap)
            .collect())
    }

    async fn key_of(&self, seq: i64) -> store::Result<Option<String>> {
        self.check(&self.fail_reads)?;
        let mut statement = self.conn.prepare(store::KEY_SQL).unwrap();
        let mut rows = statement.query_map([seq], |r| r.get(0)).unwrap();
        Ok(rows.next().map(Result::unwrap))
    }

    async fn expired(&self, cutoff: i64, limit: i64) -> store::Result<Vec<(i64, String)>> {
        self.check(&self.fail_reads)?;
        let mut statement = self.conn.prepare(store::EXPIRED_SQL).unwrap();
        Ok(statement
            .query_map(params![cutoff, limit], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect())
    }

    async fn delete(&self, seqs: &[i64]) -> store::Result<()> {
        self.check(&self.fail_deletes)?;
        self.conn
            .execute(&store::delete_sql(seqs.len()), params_from_iter(seqs))
            .map(|_| ())
            .map_err(|_| StorageError)
    }
}

#[derive(Default)]
struct Bucket {
    objects: RefCell<HashMap<String, Vec<u8>>>,
    fail_put: Cell<bool>,
    fail_get: Cell<bool>,
    fail_delete: Cell<bool>,
}

impl Bodies for Bucket {
    async fn put(&self, key: &str, bytes: &[u8]) -> store::Result<()> {
        if self.fail_put.get() {
            return Err(StorageError);
        }
        self.objects.borrow_mut().insert(key.into(), bytes.to_vec());
        Ok(())
    }

    async fn get(&self, key: &str) -> store::Result<Option<Vec<u8>>> {
        if self.fail_get.get() {
            return Err(StorageError);
        }
        Ok(self.objects.borrow().get(key).cloned())
    }

    async fn delete(&self, keys: &[String]) -> store::Result<()> {
        if self.fail_delete.get() {
            return Err(StorageError);
        }
        for key in keys {
            self.objects.borrow_mut().remove(key);
        }
        Ok(())
    }
}

struct Mail {
    from: String,
    to: String,
    raw: Vec<u8>,
    raw_size: f64,
    rejected: RefCell<Option<String>>,
}

impl Mail {
    fn new() -> Self {
        Mail::with(mime(&[], "hello").into_bytes())
    }

    fn with(raw: Vec<u8>) -> Self {
        Mail {
            from: "alice@sender.test".into(),
            to: BOT.into(),
            raw_size: raw.len() as f64,
            raw,
            rejected: RefCell::new(None),
        }
    }

    fn from(mut self, from: &str) -> Self {
        self.from = from.into();
        self
    }

    fn to(mut self, to: &str) -> Self {
        self.to = to.into();
        self
    }

    fn size(mut self, size: f64) -> Self {
        self.raw_size = size;
        self
    }

    fn rejected(&self) -> Option<String> {
        self.rejected.borrow().clone()
    }
}

impl email::Inbound for Mail {
    fn from(&self) -> String {
        self.from.clone()
    }

    fn to(&self) -> String {
        self.to.clone()
    }

    fn raw_size(&self) -> f64 {
        self.raw_size
    }

    fn set_reject(&self, reason: &str) {
        *self.rejected.borrow_mut() = Some(reason.into());
    }

    async fn read_capped(&self, max: usize) -> store::Result<Option<Vec<u8>>> {
        let mut out = Vec::new();
        for chunk in self.raw.chunks(4096) {
            if out.len() + chunk.len() > max {
                return Ok(None);
            }
            out.extend_from_slice(chunk);
        }
        Ok(Some(out))
    }
}

struct Env {
    vars: Vars,
    db: Db,
    bucket: Bucket,
    keys: Cell<u32>,
}

impl Env {
    fn new() -> Self {
        Env {
            vars: Vars {
                inbound_token: Some(TOKEN.into()),
                allowed_recipients: Some(BOT.into()),
                ..Vars::default()
            },
            db: Db::new(),
            bucket: Bucket::default(),
            keys: Cell::new(0),
        }
    }

    fn recipients(mut self, value: Option<&str>) -> Self {
        self.vars.allowed_recipients = value.map(Into::into);
        self
    }

    fn receive_at(&self, mail: &Mail, now: i64) -> Result<(), email::EmailError> {
        self.keys.set(self.keys.get() + 1);
        let key = format!("mail/{}.eml", self.keys.get());
        block_on(email::handle(
            mail,
            &self.vars,
            &self.db,
            &self.bucket,
            now,
            key,
        ))
    }

    fn receive(&self, mail: &Mail) -> Result<(), email::EmailError> {
        self.receive_at(mail, 1_700_000_000_000)
    }

    fn request(&self, method: &str, path: &str, token: Option<&str>) -> http::Response {
        let url = Url::parse(&format!("https://mail.example.com{path}")).unwrap();
        let authorization = token.map(|t| format!("Bearer {t}"));
        self.raw_request(method, &url, authorization.as_deref())
    }

    fn raw_request(&self, method: &str, url: &Url, authorization: Option<&str>) -> http::Response {
        block_on(http::handle(
            http::Request {
                method,
                url,
                authorization,
            },
            &self.vars,
            &self.db,
            &self.bucket,
        ))
    }

    fn get(&self, path: &str) -> http::Response {
        self.request("GET", path, Some(TOKEN))
    }

    fn seed(&self, count: usize, headers: &[&str]) {
        for i in 1..=count {
            let mail = Mail::with(mime(headers, &format!("message {i}")).into_bytes());
            self.receive_at(&mail, 1000 + i as i64).unwrap();
        }
    }
}

fn mime(headers: &[&str], body: &str) -> String {
    let mut lines: Vec<&str> = headers.to_vec();
    lines.extend([
        "From: Alice <alice@sender.test>",
        "To: bot@example.com",
        "Subject: hi",
        "",
        body,
        "",
    ]);
    lines.join("\r\n")
}

fn json(response: &http::Response) -> serde_json::Value {
    serde_json::from_slice(&response.body).unwrap()
}

fn ids(response: &http::Response) -> Vec<String> {
    json(response)["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect()
}

// ---- email handler: receive and store

#[test]
fn stores_raw_mime_in_r2_and_metadata_in_d1_using_the_envelope() {
    let env = Env::new();
    let raw = mime(&[CF_PASS], "body é").into_bytes();
    let mail = Mail::with(raw.clone())
        .from("Alice@Sender.Test")
        .to("BOT@example.com");
    env.receive_at(&mail, 1_700_000_000_000).unwrap();
    assert_eq!(mail.rejected(), None);
    let rows = env.db.rows();
    let (seq, key, from, to, at, size, failed) = &rows[0];
    assert_eq!(
        (*seq, from.as_str(), to.as_str(), *at),
        (1, "alice@sender.test", "bot@example.com", 1_700_000_000_000)
    );
    assert_eq!(*size, raw.len() as i64);
    assert_eq!(*failed, 0);
    assert_eq!(env.bucket.objects.borrow()[key], raw);
}

#[test]
fn ignores_header_from_and_to_because_the_envelope_is_authoritative() {
    let env = Env::new();
    let mail = Mail::with(mime(&["Return-Path: <spoof@evil.test>"], "hello").into_bytes())
        .from("real@sender.test");
    env.receive(&mail).unwrap();
    assert_eq!(env.db.rows()[0].2, "real@sender.test");
}

#[test]
fn preserves_the_bytes_of_a_multi_chunk_message_exactly() {
    let env = Env::new();
    let raw: Vec<u8> = (0..300_000u32).map(|i| ((i * 31) % 256) as u8).collect();
    env.receive(&Mail::with(raw.clone())).unwrap();
    let key = env.db.rows()[0].1.clone();
    assert_eq!(env.bucket.objects.borrow()[&key], raw);
}

// ---- email handler: allowlist and limits

#[test]
fn rejects_unknown_recipients_without_storing() {
    let env = Env::new();
    let mail = Mail::new().to("other@example.com");
    env.receive(&mail).unwrap();
    assert!(mail.rejected().unwrap().starts_with("550 "));
    assert!(env.db.rows().is_empty());
    assert!(env.bucket.objects.borrow().is_empty());
}

#[test]
fn supports_domain_entries_case_insensitively() {
    let env = Env::new().recipients(Some(" Other@Example.com , *@bots.example.com "));
    let a = Mail::new().to("x@bots.example.com");
    let b = Mail::new().to("other@example.com");
    let c = Mail::new().to("x@evil.bots.example.com.attacker.test");
    for mail in [&a, &b, &c] {
        env.receive(mail).unwrap();
    }
    assert_eq!((a.rejected(), b.rejected()), (None, None));
    assert!(c.rejected().is_some());
    assert_eq!(env.db.rows().len(), 2);
}

#[test]
fn a_subdomain_does_not_match_a_domain_entry() {
    let env = Env::new().recipients(Some("*@example.com"));
    let mail = Mail::new().to("x@sub.example.com");
    env.receive(&mail).unwrap();
    assert!(mail.rejected().is_some());
}

#[test]
fn fails_closed_when_allowed_recipients_is_missing_or_invalid() {
    for value in [
        None,
        Some(""),
        Some(" , "),
        Some("not-an-address"),
        Some("bot@example.com,*@"),
    ] {
        let env = Env::new().recipients(value);
        assert_eq!(
            env.receive(&Mail::new()),
            Err(email::EmailError::NotConfigured),
            "{value:?}"
        );
        assert!(env.db.rows().is_empty());
    }
}

#[test]
fn rejects_a_null_or_malformed_envelope_sender() {
    let env = Env::new();
    for from in ["", "<>", "no-at-sign", "a b@c.test"] {
        let mail = Mail::new().from(from);
        env.receive(&mail).unwrap();
        assert!(mail.rejected().is_some(), "{from}");
    }
    assert!(env.db.rows().is_empty());
}

#[test]
fn rejects_a_declared_size_over_2_mib_without_reading() {
    let env = Env::new();
    let mail = Mail::new().size((MAX_MESSAGE_BYTES + 1) as f64);
    env.receive(&mail).unwrap();
    assert!(mail.rejected().unwrap().starts_with("552 "));
    assert!(env.db.rows().is_empty());
}

#[test]
fn rejects_a_stream_over_2_mib_even_when_the_size_understates_it() {
    let env = Env::new();
    let mail = Mail::with(vec![0; MAX_MESSAGE_BYTES + 1]).size(10.0);
    env.receive(&mail).unwrap();
    assert!(mail.rejected().unwrap().starts_with("552 "));
    assert!(env.bucket.objects.borrow().is_empty());
}

#[test]
fn accepts_a_message_of_exactly_2_mib() {
    let env = Env::new();
    let mail = Mail::with(vec![0x61; MAX_MESSAGE_BYTES]);
    env.receive(&mail).unwrap();
    assert_eq!(mail.rejected(), None);
    assert_eq!(env.db.rows()[0].5, MAX_MESSAGE_BYTES as i64);
}

#[test]
fn rejects_a_size_that_is_not_a_safe_integer() {
    let env = Env::new();
    for size in [f64::NAN, f64::INFINITY, 1.5] {
        let mail = Mail::new().size(size);
        env.receive(&mail).unwrap();
        assert!(mail.rejected().unwrap().starts_with("552 "), "{size}");
    }
}

// ---- email handler: durability

#[test]
fn indexes_nothing_and_fails_when_r2_fails() {
    let env = Env::new();
    env.bucket.fail_put.set(true);
    assert_eq!(env.receive(&Mail::new()), Err(email::EmailError::Storage));
    assert!(env.db.rows().is_empty());
}

#[test]
fn removes_the_r2_object_and_fails_when_the_d1_insert_fails() {
    let env = Env::new();
    env.db.fail_insert.set(true);
    assert_eq!(env.receive(&Mail::new()), Err(email::EmailError::Storage));
    assert!(env.bucket.objects.borrow().is_empty());
    assert!(env.db.rows().is_empty());
}

#[test]
fn assigns_strictly_increasing_seq_values() {
    let env = Env::new();
    for _ in 0..3 {
        env.receive(&Mail::new()).unwrap();
    }
    assert_eq!(env.db.seqs(), [1, 2, 3]);
}

#[test]
fn never_reuses_a_seq_after_the_newest_row_is_deleted() {
    let env = Env::new();
    env.receive(&Mail::new()).unwrap();
    env.receive(&Mail::new()).unwrap();
    env.db.conn.execute("DELETE FROM messages", []).unwrap();
    env.receive(&Mail::new()).unwrap();
    assert_eq!(env.db.seqs(), [3]);
}

// ---- Authentication-Results handling

fn auth_failed_for(headers: &[&str], from: &str, trusted: Option<&str>) -> bool {
    let mut env = Env::new();
    env.vars.trusted_authserv_id = trusted.map(Into::into);
    env.receive(&Mail::with(mime(headers, "hello").into_bytes()).from(from))
        .unwrap();
    env.db.rows()[0].6 == 1
}

fn failed(headers: &[&str]) -> bool {
    auth_failed_for(headers, "alice@sender.test", None)
}

#[test]
fn passes_only_for_one_trusted_header_with_aligned_spf_and_dmarc() {
    assert!(!failed(&[CF_PASS]));
}

#[test]
fn fails_without_an_authentication_results_header() {
    assert!(failed(&[]));
}

#[test]
fn ignores_a_forged_header_from_an_untrusted_authserv_id() {
    assert!(failed(&[
        &CF_PASS.replace("mx.cloudflare.net;", "evil.test;")
    ]));
}

#[test]
fn fails_when_a_sender_adds_a_second_trusted_header() {
    assert!(failed(&[CF_PASS, CF_PASS]));
    let failing = CF_PASS
        .replace("spf=pass", "spf=fail")
        .replace("dmarc=pass", "dmarc=fail");
    assert!(failed(&[&failing, CF_PASS]));
    assert!(failed(&[CF_PASS, &failing]));
}

#[test]
fn fails_when_a_pass_appears_only_in_a_comment_or_the_body() {
    let commented = "Authentication-Results: mx.cloudflare.net; spf=fail (spf=pass) smtp.mailfrom=alice@sender.test; dmarc=fail (dmarc=pass)";
    assert!(failed(&[commented]));
    let env = Env::new();
    env.receive(&Mail::with(
        format!("Subject: x\r\n\r\n{CF_PASS}\r\n").into_bytes(),
    ))
    .unwrap();
    assert_eq!(env.db.rows()[0].6, 1);
}

#[test]
fn fails_when_spf_passed_for_another_domain() {
    assert!(auth_failed_for(&[CF_PASS], "alice@allowed.test", None));
}

#[test]
fn fails_without_smtp_mailfrom_or_unless_spf_and_dmarc_both_pass() {
    assert!(failed(&[
        &CF_PASS.replace(" smtp.mailfrom=alice@sender.test", "")
    ]));
    assert!(failed(&[&CF_PASS.replace("dmarc=pass", "dmarc=none")]));
    assert!(failed(&[&CF_PASS.replace("spf=pass", "spf=softfail")]));
}

#[test]
fn handles_folded_headers() {
    let folded = CF_PASS
        .replace("; dmarc", ";\r\n\tdmarc")
        .replace("; spf", ";\r\n spf");
    assert!(!failed(&[&folded]));
}

#[test]
fn honours_trusted_authserv_id() {
    assert!(auth_failed_for(
        &[CF_PASS],
        "alice@sender.test",
        Some("other.test")
    ));
    let other = CF_PASS.replace("mx.cloudflare.net;", "other.test;");
    assert!(!auth_failed_for(
        &[&other],
        "alice@sender.test",
        Some("other.test")
    ));
}

#[test]
fn fails_when_the_header_block_exceeds_64_kib() {
    let env = Env::new();
    let raw = format!("{CF_PASS}\r\nX-Pad: {}\r\n\r\nbody", "a".repeat(70_000));
    env.receive(&Mail::with(raw.into_bytes())).unwrap();
    assert_eq!(env.db.rows()[0].6, 1);
}

// ---- HTTP: authentication

#[test]
fn returns_401_without_a_token_and_leaks_nothing() {
    let env = Env::new();
    env.seed(1, &[CF_PASS]);
    for path in ["/v1/messages", "/v1/messages/1", "/unknown"] {
        let response = env.request("GET", path, None);
        assert_eq!(response.status, 401);
        assert_eq!(response.header("www-authenticate"), Some("Bearer"));
        assert_eq!(response.body, br#"{"error":"unauthorized"}"#);
    }
}

#[test]
fn returns_401_for_wrong_prefixed_or_differently_schemed_tokens() {
    let env = Env::new();
    let url = Url::parse("https://mail.example.com/v1/messages").unwrap();
    let wrong = "x".repeat(40);
    let longer = format!("Bearer {TOKEN}x");
    let shorter = format!("Bearer {}", &TOKEN[1..]);
    let basic = format!("Basic {TOKEN}");
    let extra = format!("Bearer {TOKEN} extra");
    for value in [
        wrong.as_str(),
        &longer,
        &shorter,
        &basic,
        TOKEN,
        "Bearer ",
        &extra,
    ] {
        assert_eq!(
            env.raw_request("GET", &url, Some(value)).status,
            401,
            "{value}"
        );
    }
    let lower = format!("bearer {TOKEN}");
    assert_eq!(env.raw_request("GET", &url, Some(&lower)).status, 200);
}

#[test]
fn fails_closed_with_503_when_the_token_is_absent_or_short() {
    for token in [
        None,
        Some(""),
        Some("short"),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
    ] {
        let mut env = Env::new();
        env.vars.inbound_token = token.map(Into::into);
        assert_eq!(
            env.request("GET", "/v1/messages", Some(token.unwrap_or("anything")))
                .status,
            503
        );
    }
}

#[test]
fn accepts_a_32_character_token() {
    let token = "k".repeat(32);
    let mut env = Env::new();
    env.vars.inbound_token = Some(token.clone());
    assert_eq!(env.request("GET", "/v1/messages", Some(&token)).status, 200);
}

#[test]
fn rejects_non_get_methods_after_authentication() {
    let env = Env::new();
    for method in ["POST", "PUT", "DELETE", "OPTIONS", "PATCH"] {
        let response = env.request(method, "/v1/messages", Some(TOKEN));
        assert_eq!(response.status, 405, "{method}");
        assert_eq!(response.header("allow"), Some("GET"));
    }
    assert_eq!(env.request("OPTIONS", "/v1/messages", None).status, 401);
}

#[test]
fn never_sets_cors_headers_or_redirects() {
    let env = Env::new();
    env.seed(1, &[CF_PASS]);
    for path in ["/v1/messages", "/v1/messages/1", "/nope", "/v1/messages/"] {
        let response = env.get(path);
        assert_eq!(response.header("access-control-allow-origin"), None);
        assert!(!(300..400).contains(&response.status), "{path}");
    }
}

// ---- HTTP: GET /v1/messages

#[test]
fn lists_the_documented_shape_and_cursor() {
    let env = Env::new();
    env.seed(2, &[CF_PASS]);
    let response = env.get("/v1/messages?after=0&limit=10");
    assert_eq!(response.status, 200);
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert!(response
        .header("content-type")
        .unwrap()
        .starts_with("application/json"));
    let body = json(&response);
    assert_eq!(ids(&response), ["1", "2"]);
    assert_eq!(
        body["messages"][0],
        serde_json::json!({
            "id": "1",
            "from": "alice@sender.test",
            "to": "bot@example.com",
            "receivedAt": 1001,
            "size": mime(&[CF_PASS], "message 1").len(),
            "authFailed": false,
        })
    );
    assert_eq!(body["cursor"], "2");
}

#[test]
fn pages_without_gaps_or_duplicates_and_keeps_the_cursor_when_empty() {
    let env = Env::new();
    env.seed(5, &[CF_PASS]);
    let mut seen = Vec::new();
    let mut cursor = "0".to_string();
    for _ in 0..10 {
        let response = env.get(&format!("/v1/messages?after={cursor}&limit=2"));
        let page = ids(&response);
        let next = json(&response)["cursor"].as_str().unwrap().to_string();
        if page.is_empty() {
            assert_eq!(next, cursor);
            break;
        }
        seen.extend(page);
        cursor = next;
    }
    assert_eq!(seen, ["1", "2", "3", "4", "5"]);
    assert_eq!(cursor, "5");
}

#[test]
fn defaults_to_after_0_and_limit_20() {
    let env = Env::new();
    env.seed(25, &[CF_PASS]);
    let response = env.get("/v1/messages");
    assert_eq!(ids(&response).len(), 20);
    assert_eq!(json(&response)["cursor"], "20");
}

#[test]
fn caps_a_page_at_50_messages() {
    let env = Env::new();
    env.seed(55, &[CF_PASS]);
    assert_eq!(ids(&env.get("/v1/messages?limit=50")).len(), 50);
}

#[test]
fn rejects_invalid_pagination_strictly() {
    let env = Env::new();
    for query in [
        "after=-1",
        "after=1.5",
        "after=abc",
        "after=",
        "after=01",
        "after=1e3",
        "after=%2B1",
        "after=1&after=2",
        "after=1234567890123456",
        "limit=0",
        "limit=51",
        "limit=-1",
        "limit=05",
        "limit=1.0",
        "limit=",
        "limit=10&limit=10",
        "offset=1",
        "after=0&extra=1",
    ] {
        assert_eq!(
            env.get(&format!("/v1/messages?{query}")).status,
            400,
            "{query}"
        );
    }
}

#[test]
fn a_transient_d1_failure_keeps_the_cursor_with_the_client() {
    let env = Env::new();
    env.seed(3, &[CF_PASS]);
    let first = json(&env.get("/v1/messages?limit=1"))["cursor"]
        .as_str()
        .unwrap()
        .to_string();
    env.db.fail_reads.set(true);
    let failed = env.get(&format!("/v1/messages?after={first}"));
    assert_eq!(failed.status, 500);
    assert_eq!(failed.body, br#"{"error":"internal"}"#);
    env.db.fail_reads.set(false);
    assert_eq!(
        ids(&env.get(&format!("/v1/messages?after={first}"))),
        ["2", "3"]
    );
}

#[test]
fn reports_auth_failed_for_spoofed_headers() {
    let env = Env::new();
    env.seed(1, &[CF_PASS, CF_PASS]);
    assert_eq!(
        json(&env.get("/v1/messages"))["messages"][0]["authFailed"],
        true
    );
}

// ---- HTTP: GET /v1/messages/<id>

#[test]
fn returns_the_exact_raw_mime_as_message_rfc822() {
    let env = Env::new();
    let raw: Vec<u8> = (0..100_000u32).map(|i| ((i * 7) % 256) as u8).collect();
    env.receive(&Mail::with(raw.clone())).unwrap();
    let response = env.get("/v1/messages/1");
    assert_eq!(response.status, 200);
    assert_eq!(response.header("content-type"), Some("message/rfc822"));
    assert_eq!(
        response.header("content-length"),
        Some(raw.len().to_string().as_str())
    );
    assert_eq!(response.header("cache-control"), Some("no-store"));
    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(response.body, raw);
}

#[test]
fn returns_404_for_unknown_ids_and_unsafe_ids() {
    let env = Env::new();
    env.seed(1, &[CF_PASS]);
    assert_eq!(env.get("/v1/messages/99").status, 404);
    for id in [
        "0",
        "01",
        "-1",
        "1.5",
        "abc",
        "1%2F2",
        "1/",
        "../1",
        "1234567890123456",
        "1;DROP",
        "%31",
    ] {
        assert_eq!(env.get(&format!("/v1/messages/{id}")).status, 404, "{id}");
    }
}

#[test]
fn rejects_query_strings_on_the_get_route() {
    let env = Env::new();
    env.seed(1, &[CF_PASS]);
    assert_eq!(env.get("/v1/messages/1?x=1").status, 400);
    assert_eq!(env.get("/v1/messages/1?").status, 200);
}

#[test]
fn returns_410_when_the_body_is_gone_and_500_on_storage_errors() {
    let env = Env::new();
    env.seed(1, &[CF_PASS]);
    let key = env.db.rows()[0].1.clone();
    env.bucket.objects.borrow_mut().remove(&key);
    assert_eq!(env.get("/v1/messages/1").status, 410);
    env.bucket.fail_get.set(true);
    assert_eq!(env.get("/v1/messages/1").status, 500);
}

// ---- logging

#[test]
fn never_logs_the_token_or_addresses() {
    log::LINES.with(|lines| lines.borrow_mut().clear());
    let env = Env::new();
    env.db.fail_reads.set(true);
    env.get("/v1/messages");
    let mut short = Env::new();
    short.vars.inbound_token = Some("tok-abc".into());
    short.request("GET", "/v1/messages", Some("tok-abc"));
    block_on(retention::run(&short.vars, &short.db, &short.bucket, 0)).unwrap();
    let logged = log::LINES.with(|lines| lines.borrow().join("\n"));
    assert!(!logged.is_empty());
    assert!(!logged.contains(TOKEN) && !logged.contains("tok-abc") && !logged.contains('@'));
}

// ---- retention

const NOW: i64 = 1_800_000_000_000;

fn seed_at(env: &Env, ages: &[i64]) {
    for age in ages {
        env.receive_at(&Mail::new(), NOW - age * DAY).unwrap();
    }
}

fn sweep(env: &Env) -> Result<usize, StorageError> {
    block_on(retention::run(&env.vars, &env.db, &env.bucket, NOW))
}

#[test]
fn retention_removes_expired_rows_and_bodies_and_keeps_fresh_ones() {
    let env = Env::new();
    seed_at(&env, &[40, 31, 29, 1]);
    assert_eq!(sweep(&env), Ok(2));
    assert_eq!(env.db.seqs(), [3, 4]);
    assert_eq!(env.bucket.objects.borrow().len(), 2);
}

#[test]
fn retention_honours_retention_days_and_falls_back_to_30() {
    let mut env = Env::new();
    env.vars.retention_days = Some("7".into());
    seed_at(&env, &[8, 6]);
    assert_eq!(sweep(&env), Ok(1));
    for value in ["0", "-5", "abc", "366", "", "1e2"] {
        let mut other = Env::new();
        other.vars.retention_days = Some(value.into());
        seed_at(&other, &[31, 29]);
        assert_eq!(sweep(&other), Ok(1), "{value}");
    }
}

#[test]
fn retention_drains_more_than_one_batch() {
    let env = Env::new();
    seed_at(&env, &[60; 120]);
    assert_eq!(sweep(&env), Ok(120));
    assert!(env.bucket.objects.borrow().is_empty());
}

#[test]
fn retention_retries_after_a_d1_delete_failure_without_orphaning_rows() {
    let env = Env::new();
    seed_at(&env, &[60, 60]);
    env.db.fail_deletes.set(true);
    assert!(sweep(&env).is_err());
    assert_eq!(env.db.seqs(), [1, 2]);
    env.db.fail_deletes.set(false);
    assert_eq!(sweep(&env), Ok(2));
    assert!(env.db.seqs().is_empty());
}

#[test]
fn retention_leaves_rows_when_the_r2_delete_fails() {
    let env = Env::new();
    seed_at(&env, &[60]);
    env.bucket.fail_delete.set(true);
    assert!(sweep(&env).is_err());
    assert_eq!(env.db.seqs(), [1]);
}

#[test]
fn retention_does_not_reuse_seq_values() {
    let env = Env::new();
    seed_at(&env, &[60]);
    sweep(&env).unwrap();
    env.receive_at(&Mail::new(), NOW).unwrap();
    assert_eq!(env.db.seqs(), [2]);
}
