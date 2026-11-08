//! Cloudflare runtime bindings: D1, R2, the Email Worker message and the
//! three event handlers.

use crate::config::{Vars, R2_KEY_PREFIX};
use crate::store::{self, Bodies, Index, NewRow, Row, StorageError};
use crate::{email, http, log, retention};
use futures_util::StreamExt;
use serde::Deserialize;
use worker::wasm_bindgen::JsValue;
use worker::{
    event, Bucket, Context, D1Database, Env, ForwardableEmailMessage, Headers, HttpMetadata,
    ScheduleContext, ScheduledEvent,
};

fn vars(env: &Env) -> Vars {
    let var = |name: &str| env.var(name).ok().map(|value| value.to_string());
    Vars {
        inbound_token: env
            .secret("INBOUND_TOKEN")
            .ok()
            .map(|value| value.to_string()),
        allowed_recipients: var("ALLOWED_RECIPIENTS"),
        retention_days: var("RETENTION_DAYS"),
        trusted_authserv_id: var("TRUSTED_AUTHSERV_ID"),
    }
}

fn now() -> i64 {
    worker::Date::now().as_millis() as i64
}

fn num(value: i64) -> JsValue {
    JsValue::from_f64(value as f64)
}

fn storage<T>(result: worker::Result<T>) -> store::Result<T> {
    result.map_err(|_| StorageError)
}

struct D1(D1Database);

#[derive(Deserialize)]
struct RowRecord {
    seq: i64,
    envelope_from: String,
    envelope_to: String,
    received_at: i64,
    size: i64,
    auth_failed: i64,
}

#[derive(Deserialize)]
struct KeyRecord {
    r2_key: String,
}

#[derive(Deserialize)]
struct ExpiredRecord {
    seq: i64,
    r2_key: String,
}

impl Index for D1 {
    async fn insert(&self, row: NewRow<'_>) -> store::Result<()> {
        let statement = storage(self.0.prepare(store::INSERT_SQL).bind(&[
            row.r2_key.into(),
            row.envelope_from.into(),
            row.envelope_to.into(),
            num(row.received_at),
            num(row.size),
            num(i64::from(row.auth_failed)),
        ]))?;
        storage(statement.run().await).map(|_| ())
    }

    async fn after(&self, after: i64, limit: i64) -> store::Result<Vec<Row>> {
        let statement = storage(
            self.0
                .prepare(store::AFTER_SQL)
                .bind(&[num(after), num(limit)]),
        )?;
        let records: Vec<RowRecord> = storage(storage(statement.all().await)?.results())?;
        Ok(records
            .into_iter()
            .map(|r| Row {
                seq: r.seq,
                envelope_from: r.envelope_from,
                envelope_to: r.envelope_to,
                received_at: r.received_at,
                size: r.size,
                auth_failed: r.auth_failed != 0,
            })
            .collect())
    }

    async fn key_of(&self, seq: i64) -> store::Result<Option<String>> {
        let statement = storage(self.0.prepare(store::KEY_SQL).bind(&[num(seq)]))?;
        let record: Option<KeyRecord> = storage(statement.first(None).await)?;
        Ok(record.map(|r| r.r2_key))
    }

    async fn expired(&self, cutoff: i64, limit: i64) -> store::Result<Vec<(i64, String)>> {
        let statement = storage(
            self.0
                .prepare(store::EXPIRED_SQL)
                .bind(&[num(cutoff), num(limit)]),
        )?;
        let records: Vec<ExpiredRecord> = storage(storage(statement.all().await)?.results())?;
        Ok(records.into_iter().map(|r| (r.seq, r.r2_key)).collect())
    }

    async fn delete(&self, seqs: &[i64]) -> store::Result<()> {
        let values: Vec<JsValue> = seqs.iter().copied().map(num).collect();
        let statement = storage(self.0.prepare(store::delete_sql(seqs.len())).bind(&values))?;
        storage(statement.run().await).map(|_| ())
    }
}

struct R2(Bucket);

impl Bodies for R2 {
    async fn put(&self, key: &str, bytes: &[u8]) -> store::Result<()> {
        let metadata = HttpMetadata {
            content_type: Some("message/rfc822".into()),
            ..Default::default()
        };
        storage(
            self.0
                .put(key, bytes.to_vec())
                .http_metadata(metadata)
                .execute()
                .await,
        )
        .map(|_| ())
    }

    async fn get(&self, key: &str) -> store::Result<Option<Vec<u8>>> {
        let Some(object) = storage(self.0.get(key).execute().await)? else {
            return Ok(None);
        };
        let Some(body) = object.body() else {
            return Ok(None);
        };
        storage(body.bytes().await).map(Some)
    }

    async fn delete(&self, keys: &[String]) -> store::Result<()> {
        storage(self.0.delete_multiple(keys.to_vec()).await)
    }
}

struct Message(ForwardableEmailMessage);

impl email::Inbound for Message {
    fn from(&self) -> String {
        self.0.from()
    }

    fn to(&self) -> String {
        self.0.to()
    }

    fn raw_size(&self) -> f64 {
        self.0.raw_size()
    }

    fn set_reject(&self, reason: &str) {
        self.0.set_reject(reason);
    }

    async fn read_capped(&self, max: usize) -> store::Result<Option<Vec<u8>>> {
        let mut stream = self.0.raw_byte_stream();
        let mut out = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = storage(chunk)?;
            if out.len() + chunk.len() > max {
                // Dropping the stream cancels the rest of the body.
                return Ok(None);
            }
            out.extend_from_slice(&chunk);
        }
        Ok(Some(out))
    }
}

fn bindings(env: &Env) -> worker::Result<(D1, R2)> {
    Ok((D1(env.d1("INBOUND_DB")?), R2(env.bucket("RAW_MAIL")?)))
}

#[event(fetch)]
async fn fetch(
    request: worker::Request,
    env: Env,
    _ctx: Context,
) -> worker::Result<worker::Response> {
    let (index, bodies) = bindings(&env)?;
    let url = request.url()?;
    let method = request.method().to_string();
    let authorization = request.headers().get("authorization")?;
    let response = http::handle(
        http::Request {
            method: &method,
            url: &url,
            authorization: authorization.as_deref(),
        },
        &vars(&env),
        &index,
        &bodies,
    )
    .await;
    let headers = Headers::new();
    for (name, value) in &response.headers {
        headers.set(name, value)?;
    }
    Ok(worker::Response::from_bytes(response.body)?
        .with_status(response.status)
        .with_headers(headers))
}

#[event(email)]
async fn email(message: ForwardableEmailMessage, env: Env, _ctx: Context) -> worker::Result<()> {
    let (index, bodies) = bindings(&env)?;
    let key = format!("{R2_KEY_PREFIX}{}.eml", uuid::Uuid::new_v4());
    email::handle(&Message(message), &vars(&env), &index, &bodies, now(), key)
        .await
        .map_err(|error| worker::Error::RustError(error.to_string()))
}

#[event(scheduled)]
async fn scheduled(_event: ScheduledEvent, env: Env, _ctx: ScheduleContext) {
    let Ok((index, bodies)) = bindings(&env) else {
        log::error("inbound: retention could not open its bindings");
        return;
    };
    match retention::run(&vars(&env), &index, &bodies, now()).await {
        Ok(deleted) => log::count("inbound: retention removed messages:", deleted),
        Err(_) => log::error("inbound: retention failed; it runs again tomorrow"),
    }
}
