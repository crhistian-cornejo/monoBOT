import { hasValidToken } from "./config.ts";
import type { Env } from "./types.ts";

const DEFAULT_LIMIT = 20;
const MAX_LIMIT = 50;
const MAX_ID_DIGITS = 15;

const BASE_HEADERS = {
  "cache-control": "no-store",
  "x-content-type-options": "nosniff",
} as const;

function json(status: number, body: unknown, extra: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { ...BASE_HEADERS, "content-type": "application/json; charset=utf-8", ...extra },
  });
}

function error(status: number, code: string, extra: Record<string, string> = {}): Response {
  return json(status, { error: code }, extra);
}

async function digest(value: string): Promise<Uint8Array> {
  return new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(value)));
}

async function tokenMatches(header: string | null, expected: string): Promise<boolean> {
  const match = header === null ? null : /^Bearer ([\x21-\x7e]+)$/i.exec(header);
  const presented = match?.[1];
  // Compare fixed-length digests so neither content nor length leaks via timing.
  const [a, b] = await Promise.all([digest(presented ?? ""), digest(expected)]);
  let diff = presented === undefined ? 1 : 0;
  for (let i = 0; i < a.length; i++) diff |= (a[i] ?? 0) ^ (b[i] ?? 0);
  return diff === 0;
}

interface MessageRow {
  seq: number;
  envelope_from: string;
  envelope_to: string;
  received_at: number;
  size: number;
  auth_failed: number;
}

function parseListQuery(params: URLSearchParams): { after: number; limit: number } | null {
  const seen = new Set<string>();
  let after = 0;
  let limit = DEFAULT_LIMIT;
  for (const [key, value] of params) {
    if (seen.has(key)) return null;
    seen.add(key);
    if (key === "after") {
      if (!/^(0|[1-9]\d*)$/.test(value) || value.length > MAX_ID_DIGITS) return null;
      after = Number(value);
    } else if (key === "limit") {
      if (!/^[1-9]\d?$/.test(value)) return null;
      limit = Number(value);
      if (limit > MAX_LIMIT) return null;
    } else {
      return null;
    }
  }
  return { after, limit };
}

async function listMessages(url: URL, env: Env): Promise<Response> {
  const query = parseListQuery(url.searchParams);
  if (query === null) return error(400, "invalid_query");
  const { results } = await env.INBOUND_DB.prepare(
    "SELECT seq, envelope_from, envelope_to, received_at, size, auth_failed FROM messages WHERE seq > ? ORDER BY seq ASC LIMIT ?",
  )
    .bind(query.after, query.limit)
    .all<MessageRow>();
  const last = results[results.length - 1];
  return json(200, {
    messages: results.map((row) => ({
      id: String(row.seq),
      from: row.envelope_from,
      to: row.envelope_to,
      receivedAt: row.received_at,
      size: row.size,
      authFailed: row.auth_failed !== 0,
    })),
    // The cursor only moves to a seq that was actually returned, so a failed
    // or truncated response can never advance a client past unseen messages.
    cursor: String(last?.seq ?? query.after),
  });
}

async function getMessage(id: string, env: Env): Promise<Response> {
  const row = await env.INBOUND_DB.prepare("SELECT r2_key FROM messages WHERE seq = ?")
    .bind(Number(id))
    .first<{ r2_key: string }>();
  if (row === null) return error(404, "not_found");
  const object = await env.RAW_MAIL.get(row.r2_key);
  if (object === null) return error(410, "body_expired");
  return new Response(object.body, {
    status: 200,
    headers: {
      ...BASE_HEADERS,
      "content-type": "message/rfc822",
      "content-length": String(object.size),
      "content-disposition": 'attachment; filename="message.eml"',
    },
  });
}

export async function handleRequest(request: Request, env: Env): Promise<Response> {
  if (!hasValidToken(env)) {
    console.error("inbound: INBOUND_TOKEN is missing or shorter than 32 characters");
    return error(503, "unavailable");
  }
  if (!(await tokenMatches(request.headers.get("authorization"), env.INBOUND_TOKEN))) {
    return error(401, "unauthorized", { "www-authenticate": "Bearer" });
  }
  if (request.method !== "GET") return error(405, "method_not_allowed", { allow: "GET" });

  const url = new URL(request.url);
  try {
    if (url.pathname === "/v1/messages") return await listMessages(url, env);
    const match = /^\/v1\/messages\/([1-9]\d*)$/.exec(url.pathname);
    if (match?.[1] !== undefined && match[1].length <= MAX_ID_DIGITS) {
      if (url.search !== "") return error(400, "invalid_query");
      return await getMessage(match[1], env);
    }
    return error(404, "not_found");
  } catch (cause) {
    console.error("inbound: request failed", cause instanceof Error ? cause.name : "error");
    return error(500, "internal");
  }
}
