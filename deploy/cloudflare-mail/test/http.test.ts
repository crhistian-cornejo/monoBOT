import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { handleEmail } from "../src/email.ts";
import { handleRequest } from "../src/http.ts";
import worker from "../src/index.ts";
import { CF_PASS, TOKEN, createEmail, createEnv, mime, request } from "./support.ts";

const enc = (text: string) => new TextEncoder().encode(text);

async function seed(env: ReturnType<typeof createEnv>, count: number, headers: string[] = [CF_PASS]) {
  for (let i = 1; i <= count; i++) {
    await handleEmail(createEmail({ raw: enc(mime(headers, `message ${i}`)) }), env, () => 1000 + i);
  }
}

describe("authentication", () => {
  it("returns 401 without a token and leaks nothing", async () => {
    const env = createEnv();
    await seed(env, 1);
    for (const path of ["/v1/messages", "/v1/messages/1", "/unknown"]) {
      const response = await handleRequest(request(path, { token: null }), env);
      assert.equal(response.status, 401);
      assert.equal(response.headers.get("www-authenticate"), "Bearer");
      assert.equal(await response.text(), '{"error":"unauthorized"}');
    }
  });

  it("returns 401 for wrong, prefixed, or differently-schemed tokens", async () => {
    const env = createEnv();
    for (const value of ["x".repeat(40), `${TOKEN}x`, TOKEN.slice(1), `Basic ${TOKEN}`, TOKEN, "Bearer ", `Bearer ${TOKEN} extra`]) {
      const response = await handleRequest(
        new Request("https://mail.example.com/v1/messages", { headers: { authorization: value } }),
        env,
      );
      assert.equal(response.status, 401, value);
    }
  });

  it("fails closed with 503 when INBOUND_TOKEN is absent or too short", async () => {
    for (const token of [undefined, "", "short", "a".repeat(31)]) {
      const env = createEnv();
      if (token === undefined) delete env.INBOUND_TOKEN;
      else env.INBOUND_TOKEN = token;
      const response = await handleRequest(request("/v1/messages", { token: token ?? "anything" }), env);
      assert.equal(response.status, 503);
    }
  });

  it("accepts a 32 character token", async () => {
    const token = "k".repeat(32);
    const env = createEnv({ INBOUND_TOKEN: token });
    assert.equal((await handleRequest(request("/v1/messages", { token }), env)).status, 200);
  });

  it("rejects non-GET methods after authentication", async () => {
    const env = createEnv();
    for (const method of ["POST", "PUT", "DELETE", "OPTIONS", "PATCH"]) {
      const response = await handleRequest(request("/v1/messages", { method }), env);
      assert.equal(response.status, 405, method);
      assert.equal(response.headers.get("allow"), "GET");
    }
    assert.equal((await handleRequest(request("/v1/messages", { method: "OPTIONS", token: null }), env)).status, 401);
  });

  it("never sets CORS headers or redirects", async () => {
    const env = createEnv();
    await seed(env, 1);
    for (const path of ["/v1/messages", "/v1/messages/1", "/nope", "/v1/messages/"]) {
      const response = await handleRequest(request(path, { headers: { origin: "https://evil.test" } }), env);
      assert.equal(response.headers.get("access-control-allow-origin"), null);
      assert.ok(response.status < 300 || response.status >= 400, path);
    }
  });
});

describe("GET /v1/messages", () => {
  it("returns the documented shape and cursor", async () => {
    const env = createEnv();
    await seed(env, 2);
    const response = await handleRequest(request("/v1/messages?after=0&limit=10"), env);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("cache-control"), "no-store");
    assert.match(response.headers.get("content-type") ?? "", /^application\/json/);
    const body = (await response.json()) as { messages: Array<Record<string, unknown>>; cursor: string };
    assert.deepEqual(body.messages.map((m) => m.id), ["1", "2"]);
    assert.deepEqual(Object.keys(body.messages[0] ?? {}).sort(), ["authFailed", "from", "id", "receivedAt", "size", "to"]);
    assert.deepEqual(body.messages[0], {
      id: "1",
      from: "alice@sender.test",
      to: "bot@example.com",
      receivedAt: 1001,
      size: enc(mime([CF_PASS], "message 1")).byteLength,
      authFailed: false,
    });
    assert.equal(body.cursor, "2");
  });

  it("pages without gaps or duplicates and keeps the cursor when empty", async () => {
    const env = createEnv();
    await seed(env, 5);
    const ids: string[] = [];
    let cursor = "0";
    for (let guard = 0; guard < 10; guard++) {
      const response = await handleRequest(request(`/v1/messages?after=${cursor}&limit=2`), env);
      const body = (await response.json()) as { messages: Array<{ id: string }>; cursor: string };
      if (body.messages.length === 0) {
        assert.equal(body.cursor, cursor);
        break;
      }
      ids.push(...body.messages.map((m) => m.id));
      cursor = body.cursor;
    }
    assert.deepEqual(ids, ["1", "2", "3", "4", "5"]);
    assert.equal(cursor, "5");
  });

  it("defaults after=0 and limit=20", async () => {
    const env = createEnv();
    await seed(env, 25);
    const body = (await (await handleRequest(request("/v1/messages"), env)).json()) as { messages: unknown[]; cursor: string };
    assert.equal(body.messages.length, 20);
    assert.equal(body.cursor, "20");
  });

  it("caps a page at 50 messages", async () => {
    const env = createEnv();
    await seed(env, 55);
    const body = (await (await handleRequest(request("/v1/messages?limit=50"), env)).json()) as { messages: unknown[] };
    assert.equal(body.messages.length, 50);
  });

  it("rejects invalid pagination strictly", async () => {
    const env = createEnv();
    const bad = [
      "after=-1", "after=1.5", "after=abc", "after=", "after=01", "after=1e3", "after=%2B1",
      "after=1&after=2", "after=1234567890123456", "limit=0", "limit=51", "limit=-1", "limit=05",
      "limit=1.0", "limit=", "limit=10&limit=10", "offset=1", "after=0&extra=1",
    ];
    for (const query of bad) {
      const response = await handleRequest(request(`/v1/messages?${query}`), env);
      assert.equal(response.status, 400, query);
    }
  });

  it("skips nothing across a transient D1 failure: cursor stays with the client", async () => {
    const env = createEnv();
    await seed(env, 3);
    const first = (await (await handleRequest(request("/v1/messages?limit=1"), env)).json()) as { cursor: string };
    const original = env.INBOUND_DB.prepare.bind(env.INBOUND_DB);
    env.INBOUND_DB.prepare = () => {
      throw new Error("D1 unavailable");
    };
    const failed = await handleRequest(request(`/v1/messages?after=${first.cursor}`), env);
    assert.equal(failed.status, 500);
    assert.equal(await failed.text(), '{"error":"internal"}');
    env.INBOUND_DB.prepare = original;
    const retry = (await (await handleRequest(request(`/v1/messages?after=${first.cursor}`), env)).json()) as {
      messages: Array<{ id: string }>;
    };
    assert.deepEqual(retry.messages.map((m) => m.id), ["2", "3"]);
  });

  it("reports authFailed true for spoofed headers", async () => {
    const env = createEnv();
    await seed(env, 1, [CF_PASS, CF_PASS]);
    const body = (await (await handleRequest(request("/v1/messages"), env)).json()) as { messages: Array<{ authFailed: boolean }> };
    assert.equal(body.messages[0]?.authFailed, true);
  });
});

describe("GET /v1/messages/<id>", () => {
  it("returns the exact raw MIME bytes as message/rfc822", async () => {
    const env = createEnv();
    const raw = new Uint8Array(100_000).map((_, i) => (i * 7) % 256);
    await handleEmail(createEmail({ raw }), env);
    const response = await handleRequest(request("/v1/messages/1"), env);
    assert.equal(response.status, 200);
    assert.equal(response.headers.get("content-type"), "message/rfc822");
    assert.equal(response.headers.get("content-length"), String(raw.byteLength));
    assert.equal(response.headers.get("cache-control"), "no-store");
    assert.equal(response.headers.get("x-content-type-options"), "nosniff");
    assert.deepEqual(new Uint8Array(await response.arrayBuffer()), raw);
  });

  it("404s for unknown ids and rejects unsafe ids", async () => {
    const env = createEnv();
    await seed(env, 1);
    assert.equal((await handleRequest(request("/v1/messages/99"), env)).status, 404);
    for (const id of ["0", "01", "-1", "1.5", "abc", "1%2F2", "1/", "../1", "1234567890123456", "1;DROP", "%31"]) {
      const response = await handleRequest(request(`/v1/messages/${id}`), env);
      assert.equal(response.status, 404, id);
    }
  });

  it("rejects query strings on the get route", async () => {
    const env = createEnv();
    await seed(env, 1);
    assert.equal((await handleRequest(request("/v1/messages/1?x=1"), env)).status, 400);
  });

  it("returns 410 when the body was removed and 500 on storage errors", async () => {
    const env = createEnv();
    await seed(env, 1);
    const key = (env.INBOUND_DB.raw.prepare("SELECT r2_key FROM messages").get() as { r2_key: string }).r2_key;
    env.RAW_MAIL.objects.delete(key);
    assert.equal((await handleRequest(request("/v1/messages/1"), env)).status, 410);
    env.RAW_MAIL.get = async () => {
      throw new Error("R2 unavailable");
    };
    assert.equal((await handleRequest(request("/v1/messages/1"), env)).status, 500);
  });
});

describe("logging", () => {
  it("never logs the token or addresses", async () => {
    const env = createEnv();
    const logged: unknown[][] = [];
    const original = { error: console.error, log: console.log };
    console.error = (...args: unknown[]) => void logged.push(args);
    console.log = (...args: unknown[]) => void logged.push(args);
    try {
      env.INBOUND_DB.prepare = () => {
        throw new Error(`boom ${TOKEN}`);
      };
      await handleRequest(request("/v1/messages"), env);
      const short = createEnv({ INBOUND_TOKEN: "tok-abc" });
      await handleRequest(request("/v1/messages", { token: "tok-abc" }), short);
      await worker.scheduled({ scheduledTime: 0, cron: "" }, createEnv());
    } finally {
      console.error = original.error;
      console.log = original.log;
    }
    assert.ok(logged.length > 0);
    assert.doesNotMatch(JSON.stringify(logged), new RegExp(`${TOKEN}|tok-abc|@`));
  });
});
