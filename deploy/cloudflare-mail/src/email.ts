import { isAuthFailed } from "./auth-results.ts";
import {
  DEFAULT_AUTHSERV_ID,
  MAX_MESSAGE_BYTES,
  R2_KEY_PREFIX,
  isAllowedRecipient,
  normalizeEnvelopeAddress,
  parseAllowlist,
} from "./config.ts";
import type { Clock, Env, InboundEmail } from "./types.ts";

/** Reads at most `max` bytes; returns null (and cancels the stream) if exceeded. */
export async function readCapped(
  stream: ReadableStream<Uint8Array>,
  max: number,
): Promise<Uint8Array | null> {
  const reader = stream.getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    total += value.byteLength;
    if (total > max) {
      await reader.cancel().catch(() => undefined);
      return null;
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let offset = 0;
  for (const chunk of chunks) {
    out.set(chunk, offset);
    offset += chunk.byteLength;
  }
  return out;
}

/**
 * Persists one inbound message: raw MIME to R2 first, then the D1 index row.
 *
 * Permanent refusals use setReject. Infrastructure failures throw so Cloudflare
 * does not report the message as accepted; an index row is only ever written
 * after its body is durable, so the pull API never lists a message without a
 * body. A crash between the two writes leaves an unindexed R2 object, which the
 * bucket lifecycle rule removes.
 */
export async function handleEmail(
  message: InboundEmail,
  env: Env,
  now: Clock = Date.now,
): Promise<void> {
  const allowlist = parseAllowlist(env.ALLOWED_RECIPIENTS);
  if (allowlist === null) {
    console.error("inbound: ALLOWED_RECIPIENTS is missing or invalid");
    throw new Error("ALLOWED_RECIPIENTS is not configured");
  }

  const to = normalizeEnvelopeAddress(message.to);
  if (to === null || !isAllowedRecipient(allowlist, to)) {
    message.setReject("550 5.1.1 Unknown recipient");
    return;
  }
  const from = normalizeEnvelopeAddress(message.from);
  if (from === null) {
    message.setReject("550 5.7.1 Sender not accepted");
    return;
  }
  if (!Number.isSafeInteger(message.rawSize) || message.rawSize > MAX_MESSAGE_BYTES) {
    message.setReject("552 5.3.4 Message too large");
    return;
  }

  const raw = await readCapped(message.raw, MAX_MESSAGE_BYTES);
  if (raw === null) {
    message.setReject("552 5.3.4 Message too large");
    return;
  }

  const authFailed = isAuthFailed(raw, from, env.TRUSTED_AUTHSERV_ID ?? DEFAULT_AUTHSERV_ID);
  const key = `${R2_KEY_PREFIX}${crypto.randomUUID()}.eml`;

  await env.RAW_MAIL.put(key, raw, { httpMetadata: { contentType: "message/rfc822" } });
  try {
    await env.INBOUND_DB.prepare(
      "INSERT INTO messages (r2_key, envelope_from, envelope_to, received_at, size, auth_failed) VALUES (?, ?, ?, ?, ?, ?)",
    )
      .bind(key, from, to, now(), raw.byteLength, authFailed ? 1 : 0)
      .run();
  } catch (error) {
    await env.RAW_MAIL.delete(key).catch(() => undefined);
    throw error;
  }
}
