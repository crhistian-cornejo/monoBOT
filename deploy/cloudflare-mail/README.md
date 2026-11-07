# MonoBOT inbound mail Worker (optional)

A small Cloudflare Worker that receives mail through Cloudflare Email Routing,
stores it durably, and lets the desktop app pull it later over HTTPS. Nothing
here runs unless you deploy it; MonoBOT works without it.

```
sender -> Cloudflare Email Routing -> Worker email() -> R2 (raw MIME) + D1 (index)
desktop app  --GET /v1/messages?after=N-->  Worker fetch()  -> D1
desktop app  --GET /v1/messages/<id>---->   Worker fetch()  -> R2
cron (daily)                                 Worker scheduled() -> retention
```

The desktop never needs to be online when mail arrives: messages wait in D1/R2
until the app asks for everything after its last cursor.

## Protocol

All requests require `Authorization: Bearer <INBOUND_TOKEN>`. Only `GET` exists.
Responses carry `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`.
There are no redirects and no CORS headers.

### `GET /v1/messages?after=<integer>&limit=<1..50>`

- `after`: optional, default `0`, decimal digits without leading zeros, at most
  15 digits. Returns messages with `id > after`, oldest first.
- `limit`: optional, default `20`, `1..50`.
- Any other parameter, a repeated parameter, or a malformed value gives `400`.

```json
{
  "messages": [
    {
      "id": "17",
      "from": "alice@sender.example",
      "to": "bot@example.com",
      "receivedAt": 1700000000000,
      "size": 4821,
      "authFailed": false
    }
  ],
  "cursor": "17"
}
```

- `id`: numeric string, the D1 sequence number. Strictly increasing, never reused
  (`AUTOINCREMENT`), assigned by a single SQLite writer, so insertion order and
  id order agree and a later insert never gets a lower id than one a client has
  already seen.
- `from` / `to`: SMTP envelope `MAIL FROM` / `RCPT TO`, lowercased. The
  `From:`/`To:` headers inside the MIME are never used for routing or storage
  metadata.
- `receivedAt`: Unix epoch milliseconds when the Worker stored it.
- `size`: raw MIME bytes (at most 2 097 152).
- `cursor`: id of the last returned message, or the request's `after` when the
  page is empty. Persist it only after you have stored the page. A failed
  request (`5xx`, network error) never moves it, so a retry with the old cursor
  returns the same messages.

### `GET /v1/messages/<id>`

`<id>` matches `[1-9][0-9]{0,14}`. Returns the exact stored bytes with
`Content-Type: message/rfc822`.

### Status codes

| Code | Meaning |
| --- | --- |
| 200 | OK |
| 400 | Invalid query string |
| 401 | Missing or wrong bearer token (empty body, no private data) |
| 404 | Unknown path, invalid id, or no such message |
| 405 | Method other than `GET` (sent only after authentication) |
| 410 | Index row exists but the body is gone (R2 expired or deleted) |
| 500 | D1/R2 failure; retry with the same cursor |
| 503 | `INBOUND_TOKEN` missing or shorter than 32 characters (fail closed) |

## Inbound rules

- `ALLOWED_RECIPIENTS` is required: comma separated exact addresses or
  `*@domain` (exact domain, not subdomains). Empty or invalid means the
  handler throws instead of accepting anything.
- Unknown recipient: `setReject("550 5.1.1 ...")`. Null/malformed envelope
  sender (`<>` bounces included): rejected `550 5.7.1`.
- Larger than 2 MiB: rejected `552 5.3.4`. Checked against `rawSize` first and
  again while streaming, so an understated size cannot slip through.
- Order of writes: R2 object first, D1 row second. The pull API therefore never
  lists a message without a body. If the D1 insert fails the R2 object is
  deleted and the handler throws, so the message is not reported as accepted.
  A crash between the two writes leaves an unindexed object that the lifecycle
  rule below removes.

## `authFailed` and bot activation

`authFailed` is deliberately conservative: it is `false` only when all hold.

1. Exactly one `Authentication-Results` header names the trusted
   authserv-id (`TRUSTED_AUTHSERV_ID`, default `mx.cloudflare.net`).
2. In it, `spf=pass` with `smtp.mailfrom` on the same domain as the envelope
   sender, and `dmarc=pass`.
3. The header block is under 64 KiB.

Everything inside the raw message is attacker-controlled, so content cannot
prove authenticity. The defence is structural: Cloudflare prepends its own
header, so a forged header with the trusted id produces a second match and
forces `authFailed: true`; headers with other authserv-ids are ignored; text in
comments or the body is ignored.

Consequences for the client (the Rust side should implement all of these):

- Activate a bot only when the envelope `from` is in an explicit sender
  allowlist **and** `authFailed` is `false`. `authFailed: false` is necessary
  but never a substitute for the allowlist.
- Treat `authFailed: true` as "unverified", not as "forged": senders without
  DMARC, forwarded mail, or a format change make it `true`.
- Do not parse `Authentication-Results` from the downloaded MIME again.

## Deploy

Prerequisites: a Cloudflare account, a domain on Cloudflare DNS, Node 22+.
Commands run from `deploy/cloudflare-mail`. No account id is stored in files;
wrangler uses your login (or `CLOUDFLARE_ACCOUNT_ID`).

```sh
npm install                       # typescript, @types/node, wrangler (this folder only)
npm run check                     # typecheck + tests

npx wrangler login
cp wrangler.example.toml wrangler.toml   # git-ignored

# 1. Storage
npx wrangler d1 create monobot-inbound-mail
#    paste the printed database_id into wrangler.toml (d1_databases)
npx wrangler r2 bucket create monobot-inbound-mail-raw

# 2. Edit wrangler.toml: ALLOWED_RECIPIENTS, RETENTION_DAYS, the route for the API
#    host (uncomment `routes`, custom_domain = true), then apply the schema.
npx wrangler d1 migrations apply INBOUND_DB --remote

# 3. R2 lifecycle: expire raw objects 7 days AFTER RETENTION_DAYS (30 + 7 = 37)
npx wrangler r2 bucket lifecycle add monobot-inbound-mail-raw expire-raw-mail mail/ --expire-days 37
#    (flags differ between wrangler versions; check `npx wrangler r2 bucket lifecycle add --help`)

# 4. Generate a random token (at least 32 characters) in your password manager.
#    Paste the same token into this interactive secret prompt and MonoBOT's
#    protected inbound-token field. Never paste it into chat or a config file.
npx wrangler secret put INBOUND_TOKEN

# 5. Deploy
npx wrangler deploy
```

Routing (dashboard, since Email Routing rules bind to a Worker there):

1. Cloudflare dashboard, your domain, **Email > Email Routing**. Enable it and
   accept the MX/SPF records it proposes.
2. **Routing rules > Create address**: custom address `bot@yourdomain`, action
   **Send to a Worker**, destination `monobot-inbound-mail`.
3. Keep the catch-all rule disabled or set to drop; the Worker also enforces
   `ALLOWED_RECIPIENTS`.
4. Send a test mail, then:

```sh
# INBOUND_TOKEN is supplied by your password manager, not printed or committed.
# Read the header on stdin so the expanded token is not a process argument.
curl -sS --header @- "https://mail-api.yourdomain/v1/messages?after=0&limit=5" <<EOF
Authorization: Bearer ${INBOUND_TOKEN}
EOF
```

Check that the first message shows `authFailed: false` for a sender that passes
SPF and DMARC. If it is always `true`, inspect the raw message (fetch `/v1/messages/1`)
and compare its `Authentication-Results` header with the format in
`src/auth-results.ts`; do not loosen the check without understanding why.

### Retention

The daily cron deletes messages older than `RETENTION_DAYS` (default 30, valid
`1..365`, anything else falls back to 30), oldest first, at most 2000 per run.
R2 bodies are deleted before their D1 rows so a failed run is simply retried.
The R2 lifecycle rule is the safety net for orphaned objects and must stay longer
than `RETENTION_DAYS`; if it were shorter, bodies would vanish while still
indexed (clients would get `410`).

A desktop that stays offline longer than `RETENTION_DAYS` loses the oldest
messages. Choose the retention accordingly.

### Operations

- Rotate the token: generate a new value in your password manager, run `npx wrangler secret put INBOUND_TOKEN`, then update the desktop.
- Secrets are never logged; logs hold only fixed messages, counts, and error class names.
- Large mail floods will fill D1/R2 until retention runs. Keep `ALLOWED_RECIPIENTS`
  tight and use Cloudflare rate limiting or WAF on the API host if exposed publicly.

## Develop

```sh
npm run typecheck   # tsc, strict
npm test            # node --test, TypeScript type stripping, node:sqlite as D1
```

Tests run the real migration on an in-memory SQLite and fake R2/Email message
objects. They cover receive/store, byte-exact retrieval, auth and fail-closed
token, strict pagination, cursor stability across failures, allowlist, size
limits, `Authentication-Results` spoofing, durability on partial failure, id
reuse, retention, and log hygiene.

## Source references

- Integration guide and all remaining account/deployment work: [MonoBOT mail workspace](../../docs/native/mail-workspace.md).
- Reference project (used through its public MCP interface, not copied): https://github.com/hieunc229/mailflare

- Email Workers runtime API (`email()` handler, `from`, `to`, `raw`, `rawSize`, `setReject`): https://developers.cloudflare.com/email-routing/email-workers/runtime-api/
- Enable Email Workers / bind a Worker to an address: https://developers.cloudflare.com/email-routing/email-workers/enable-email-workers/
- Email authentication (SPF/DKIM/DMARC) in Email Routing: https://developers.cloudflare.com/email-routing/postmaster/
- D1 migrations and API: https://developers.cloudflare.com/d1/reference/migrations/ , https://developers.cloudflare.com/d1/worker-api/
- R2 Workers API and lifecycle rules: https://developers.cloudflare.com/r2/api/workers/workers-api-reference/ , https://developers.cloudflare.com/r2/buckets/object-lifecycles/
- Cron Triggers: https://developers.cloudflare.com/workers/configuration/cron-triggers/
- Wrangler configuration and secrets: https://developers.cloudflare.com/workers/wrangler/configuration/ , https://developers.cloudflare.com/workers/configuration/secrets/
- Authentication-Results header format: RFC 8601
