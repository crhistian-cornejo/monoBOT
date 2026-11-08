//! The pull API: `GET /v1/messages` and `GET /v1/messages/<id>`.

use crate::config::Vars;
use crate::log;
use crate::store::{Bodies, Index, StorageError};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use url::Url;

const DEFAULT_LIMIT: i64 = 20;
const MAX_LIMIT: i64 = 50;
const MAX_ID_DIGITS: usize = 15;

pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a Url,
    pub authorization: Option<&'a str>,
}

#[derive(Debug)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(&'static str, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

fn base_headers() -> Vec<(&'static str, String)> {
    vec![
        ("cache-control", "no-store".into()),
        ("x-content-type-options", "nosniff".into()),
    ]
}

fn json(status: u16, body: &impl Serialize, extra: &[(&'static str, &str)]) -> Response {
    let mut headers = base_headers();
    headers.push(("content-type", "application/json; charset=utf-8".into()));
    headers.extend(extra.iter().map(|(k, v)| (*k, v.to_string())));
    Response {
        status,
        headers,
        body: serde_json::to_vec(body).unwrap_or_default(),
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

fn error(status: u16, code: &str, extra: &[(&'static str, &str)]) -> Response {
    json(status, &ErrorBody { error: code }, extra)
}

/// `Bearer <visible ASCII>`, compared as fixed-length digests so neither the
/// token's content nor its length leaks through timing.
fn token_matches(header: Option<&str>, expected: &str) -> bool {
    let presented = header.and_then(|value| {
        let (scheme, token) = value.split_at_checked(7)?;
        (scheme.eq_ignore_ascii_case("bearer ")
            && !token.is_empty()
            && token.bytes().all(|b| (0x21..=0x7e).contains(&b)))
        .then_some(token)
    });
    let a = Sha256::digest(presented.unwrap_or("").as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    let mut diff = u8::from(presented.is_none());
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn is_decimal(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit())
}

fn parse_list_query(url: &Url) -> Option<(i64, i64)> {
    let mut seen = HashSet::new();
    let (mut after, mut limit) = (0, DEFAULT_LIMIT);
    for (key, value) in url.query_pairs() {
        if !seen.insert(key.to_string()) {
            return None;
        }
        match key.as_ref() {
            "after" => {
                let canonical = value == "0" || !value.starts_with('0');
                if !is_decimal(&value) || !canonical || value.len() > MAX_ID_DIGITS {
                    return None;
                }
                after = value.parse().ok()?;
            }
            "limit" => {
                if !is_decimal(&value) || value.len() > 2 || value.starts_with('0') {
                    return None;
                }
                limit = value.parse().ok()?;
                if limit > MAX_LIMIT {
                    return None;
                }
            }
            _ => return None,
        }
    }
    Some((after, limit))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Message {
    id: String,
    from: String,
    to: String,
    received_at: i64,
    size: i64,
    auth_failed: bool,
}

#[derive(Serialize)]
struct Page {
    messages: Vec<Message>,
    cursor: String,
}

async fn list(url: &Url, index: &impl Index) -> Result<Response, StorageError> {
    let Some((after, limit)) = parse_list_query(url) else {
        return Ok(error(400, "invalid_query", &[]));
    };
    let rows = index.after(after, limit).await?;
    // The cursor only moves to a seq that was actually returned, so a failed
    // or truncated response can never advance a client past unseen messages.
    let cursor = rows.last().map_or(after, |row| row.seq).to_string();
    let messages = rows
        .into_iter()
        .map(|row| Message {
            id: row.seq.to_string(),
            from: row.envelope_from,
            to: row.envelope_to,
            received_at: row.received_at,
            size: row.size,
            auth_failed: row.auth_failed,
        })
        .collect();
    Ok(json(200, &Page { messages, cursor }, &[]))
}

async fn get(seq: i64, index: &impl Index, bodies: &impl Bodies) -> Result<Response, StorageError> {
    let Some(key) = index.key_of(seq).await? else {
        return Ok(error(404, "not_found", &[]));
    };
    let Some(body) = bodies.get(&key).await? else {
        return Ok(error(410, "body_expired", &[]));
    };
    let mut headers = base_headers();
    headers.push(("content-type", "message/rfc822".into()));
    headers.push(("content-length", body.len().to_string()));
    headers.push((
        "content-disposition",
        r#"attachment; filename="message.eml""#.into(),
    ));
    Ok(Response {
        status: 200,
        headers,
        body,
    })
}

/// `/v1/messages/<id>` with a canonical id of at most 15 digits.
fn message_id(path: &str) -> Option<i64> {
    let id = path.strip_prefix("/v1/messages/")?;
    (is_decimal(id) && !id.starts_with('0') && id.len() <= MAX_ID_DIGITS)
        .then(|| id.parse().ok())
        .flatten()
}

pub async fn handle(
    request: Request<'_>,
    vars: &Vars,
    index: &impl Index,
    bodies: &impl Bodies,
) -> Response {
    let Some(token) = vars.valid_token() else {
        log::error("inbound: INBOUND_TOKEN is missing or shorter than 32 characters");
        return error(503, "unavailable", &[]);
    };
    if !token_matches(request.authorization, token) {
        return error(401, "unauthorized", &[("www-authenticate", "Bearer")]);
    }
    if request.method != "GET" {
        return error(405, "method_not_allowed", &[("allow", "GET")]);
    }
    let url = request.url;
    let result = if url.path() == "/v1/messages" {
        list(url, index).await
    } else if let Some(seq) = message_id(url.path()) {
        if url.query().is_some_and(|query| !query.is_empty()) {
            return error(400, "invalid_query", &[]);
        }
        get(seq, index, bodies).await
    } else {
        return error(404, "not_found", &[]);
    };
    result.unwrap_or_else(|StorageError| {
        log::error("inbound: request failed");
        error(500, "internal", &[])
    })
}
