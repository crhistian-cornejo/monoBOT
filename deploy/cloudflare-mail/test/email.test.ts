import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { handleEmail } from "../src/email.ts";
import { MAX_MESSAGE_BYTES } from "../src/config.ts";
import { CF_PASS, createEmail, createEnv, mime, streamOf } from "./support.ts";

const enc = (text: string) => new TextEncoder().encode(text);

function rows(env: ReturnType<typeof createEnv>) {
  return env.INBOUND_DB.raw.prepare("SELECT * FROM messages ORDER BY seq").all() as Array<Record<string, unknown>>;
}

describe("email handler: receive and store", () => {
  it("stores raw MIME in R2 and indexed metadata in D1 using the envelope", async () => {
    const env = createEnv();
    const raw = enc(mime([CF_PASS], "body \u00e9"));
    const email = createEmail({ from: "Alice@Sender.Test", to: "BOT@example.com", raw });
    await handleEmail(email, env, () => 1_700_000_000_000);

    assert.equal(email.rejected, null);
    const [row] = rows(env);
    assert.ok(row);
    assert.equal(row.seq, 1);
    assert.equal(row.envelope_from, "alice@sender.test");
    assert.equal(row.envelope_to, "bot@example.com");
    assert.equal(row.received_at, 1_700_000_000_000);
    assert.equal(row.size, raw.byteLength);
    assert.equal(row.auth_failed, 0);
    assert.deepEqual(env.RAW_MAIL.objects.get(String(row.r2_key)), raw);
  });

  it("ignores the From/To headers: envelope is authoritative", async () => {
    const env = createEnv();
    const raw = enc(mime(["Return-Path: <spoof@evil.test>"]));
    await handleEmail(createEmail({ from: "real@sender.test", raw }), env);
    assert.equal(rows(env)[0]?.envelope_from, "real@sender.test");
  });

  it("preserves bytes of a multi-chunk message exactly", async () => {
    const env = createEnv();
    const raw = new Uint8Array(300_000).map((_, i) => (i * 31) % 256);
    await handleEmail(createEmail({ raw }), env);
    const key = String(rows(env)[0]?.r2_key);
    assert.deepEqual(env.RAW_MAIL.objects.get(key), raw);
  });
});

describe("email handler: allowlist and limits", () => {
  it("rejects unknown recipients without storing", async () => {
    const env = createEnv();
    const email = createEmail({ to: "other@example.com" });
    await handleEmail(email, env);
    assert.match(email.rejected ?? "", /^550 /);
    assert.equal(rows(env).length, 0);
    assert.equal(env.RAW_MAIL.objects.size, 0);
  });

  it("supports *@domain entries and is case-insensitive", async () => {
    const env = createEnv({ ALLOWED_RECIPIENTS: " Other@Example.com , *@bots.example.com " });
    const a = createEmail({ to: "x@bots.example.com" });
    const b = createEmail({ to: "other@example.com" });
    const c = createEmail({ to: "x@evil.bots.example.com.attacker.test" });
    for (const email of [a, b, c]) await handleEmail(email, env);
    assert.equal(a.rejected, null);
    assert.equal(b.rejected, null);
    assert.notEqual(c.rejected, null);
    assert.equal(rows(env).length, 2);
  });

  it("does not let a subdomain match a *@domain entry", async () => {
    const env = createEnv({ ALLOWED_RECIPIENTS: "*@example.com" });
    const email = createEmail({ to: "x@sub.example.com" });
    await handleEmail(email, env);
    assert.notEqual(email.rejected, null);
  });

  for (const value of [undefined, "", " , ", "not-an-address", "bot@example.com,*@"]) {
    it(`fails closed when ALLOWED_RECIPIENTS is ${JSON.stringify(value)}`, async () => {
      const env = createEnv(value === undefined ? {} : { ALLOWED_RECIPIENTS: value });
      if (value === undefined) delete env.ALLOWED_RECIPIENTS;
      await assert.rejects(handleEmail(createEmail(), env));
      assert.equal(rows(env).length, 0);
    });
  }

  it("rejects a null or malformed envelope sender", async () => {
    const env = createEnv();
    for (const from of ["", "<>", "no-at-sign", "a b@c.test"]) {
      const email = createEmail({ from });
      await handleEmail(email, env);
      assert.notEqual(email.rejected, null, from);
    }
    assert.equal(rows(env).length, 0);
  });

  it("rejects messages whose declared size exceeds 2 MiB without reading them", async () => {
    const env = createEnv();
    const email = createEmail({ rawSize: MAX_MESSAGE_BYTES + 1 });
    await handleEmail(email, env);
    assert.match(email.rejected ?? "", /^552 /);
    assert.equal(rows(env).length, 0);
  });

  it("rejects when the stream exceeds 2 MiB even if rawSize understates it", async () => {
    const env = createEnv();
    const raw = new Uint8Array(MAX_MESSAGE_BYTES + 1);
    const email = createEmail({ raw, rawSize: 10 });
    await handleEmail(email, env);
    assert.match(email.rejected ?? "", /^552 /);
    assert.equal(env.RAW_MAIL.objects.size, 0);
  });

  it("accepts a message of exactly 2 MiB", async () => {
    const env = createEnv();
    const raw = new Uint8Array(MAX_MESSAGE_BYTES).fill(0x61);
    const email = createEmail({ raw });
    await handleEmail(email, env);
    assert.equal(email.rejected, null);
    assert.equal(rows(env)[0]?.size, MAX_MESSAGE_BYTES);
  });
});

describe("email handler: durability", () => {
  it("throws and indexes nothing when R2 fails", async () => {
    const env = createEnv();
    env.RAW_MAIL.failPut = true;
    await assert.rejects(handleEmail(createEmail(), env));
    assert.equal(rows(env).length, 0);
  });

  it("removes the R2 object and throws when the D1 insert fails", async () => {
    const env = createEnv();
    env.INBOUND_DB.failNextRun = true;
    await assert.rejects(handleEmail(createEmail(), env), /D1 unavailable/);
    assert.equal(env.RAW_MAIL.objects.size, 0);
    assert.equal(rows(env).length, 0);
  });

  it("assigns strictly increasing seq values", async () => {
    const env = createEnv();
    for (let i = 0; i < 3; i++) await handleEmail(createEmail(), env);
    assert.deepEqual(rows(env).map((r) => r.seq), [1, 2, 3]);
  });

  it("never reuses a seq after the newest row is deleted", async () => {
    const env = createEnv();
    await handleEmail(createEmail(), env);
    await handleEmail(createEmail(), env);
    env.INBOUND_DB.raw.exec("DELETE FROM messages");
    await handleEmail(createEmail(), env);
    assert.equal(rows(env)[0]?.seq, 3);
  });
});

describe("email handler: Authentication-Results handling", () => {
  async function authFailedFor(headers: string[], from = "alice@sender.test", trusted?: string) {
    const env = createEnv(trusted === undefined ? {} : { TRUSTED_AUTHSERV_ID: trusted });
    await handleEmail(createEmail({ from, raw: enc(mime(headers)) }), env);
    return rows(env)[0]?.auth_failed === 1;
  }

  it("is false only for one trusted header proving aligned SPF and DMARC", async () => {
    assert.equal(await authFailedFor([CF_PASS]), false);
  });

  it("is true when no Authentication-Results header exists", async () => {
    assert.equal(await authFailedFor([]), true);
  });

  it("ignores a forged header from an untrusted authserv-id", async () => {
    const forged = CF_PASS.replace("mx.cloudflare.net;", "evil.test;");
    assert.equal(await authFailedFor([forged]), true);
  });

  it("is true when a sender adds a second header claiming the trusted authserv-id", async () => {
    assert.equal(await authFailedFor([CF_PASS, CF_PASS]), true);
    const failing = CF_PASS.replace("spf=pass", "spf=fail").replace("dmarc=pass", "dmarc=fail");
    assert.equal(await authFailedFor([failing, CF_PASS]), true);
    assert.equal(await authFailedFor([CF_PASS, failing]), true);
  });

  it("is true when a pass appears only inside a comment or the body", async () => {
    const commented = "Authentication-Results: mx.cloudflare.net; spf=fail (spf=pass) smtp.mailfrom=alice@sender.test; dmarc=fail (dmarc=pass)";
    assert.equal(await authFailedFor([commented]), true);
    const env = createEnv();
    const raw = enc(`Subject: x\r\n\r\n${CF_PASS}\r\n`);
    await handleEmail(createEmail({ raw }), env);
    assert.equal(rows(env)[0]?.auth_failed, 1);
  });

  it("is true when SPF passed for a different domain than the envelope sender", async () => {
    assert.equal(await authFailedFor([CF_PASS], "alice@allowed.test"), true);
  });

  it("is true when smtp.mailfrom is missing, or SPF/DMARC do not both pass", async () => {
    assert.equal(await authFailedFor([CF_PASS.replace(" smtp.mailfrom=alice@sender.test", "")]), true);
    assert.equal(await authFailedFor([CF_PASS.replace("dmarc=pass", "dmarc=none")]), true);
    assert.equal(await authFailedFor([CF_PASS.replace("spf=pass", "spf=softfail")]), true);
  });

  it("handles folded headers", async () => {
    const folded = CF_PASS.replace("; dmarc", ";\r\n\tdmarc").replace("; spf", ";\r\n spf");
    assert.equal(await authFailedFor([folded]), false);
  });

  it("honours TRUSTED_AUTHSERV_ID", async () => {
    assert.equal(await authFailedFor([CF_PASS], "alice@sender.test", "other.test"), true);
    assert.equal(
      await authFailedFor([CF_PASS.replace("mx.cloudflare.net;", "other.test;")], "alice@sender.test", "other.test"),
      false,
    );
  });

  it("is true when the header block exceeds 64 KiB", async () => {
    const env = createEnv();
    const raw = enc(`${CF_PASS}\r\nX-Pad: ${"a".repeat(70_000)}\r\n\r\nbody`);
    await handleEmail(createEmail({ raw }), env);
    assert.equal(rows(env)[0]?.auth_failed, 1);
  });
});

describe("helpers", () => {
  it("streamOf round-trips", async () => {
    const bytes = new Uint8Array(10_000).fill(7);
    const chunks: Uint8Array[] = [];
    for await (const chunk of streamOf(bytes, 999)) chunks.push(chunk);
    assert.equal(chunks.reduce((n, c) => n + c.byteLength, 0), 10_000);
  });
});
