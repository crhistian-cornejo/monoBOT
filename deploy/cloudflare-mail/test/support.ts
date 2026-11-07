import { readFileSync } from "node:fs";
import { DatabaseSync } from "node:sqlite";
import type {
  D1Database,
  D1PreparedStatement,
  D1Value,
  Env,
  InboundEmail,
  R2Bucket,
} from "../src/types.ts";

export const TOKEN = "t".repeat(40);
export const BOT = "bot@example.com";

const MIGRATION = readFileSync(new URL("../migrations/0001_init.sql", import.meta.url), "utf8");

export interface FakeDb extends D1Database {
  raw: DatabaseSync;
  failNextRun: boolean;
}

/** Real SQLite running the real migration, behind the D1 surface the Worker uses. */
export function createDb(): FakeDb {
  const raw = new DatabaseSync(":memory:");
  raw.exec(MIGRATION);
  const db: FakeDb = {
    raw,
    failNextRun: false,
    prepare(sql: string): D1PreparedStatement {
      let values: D1Value[] = [];
      const statement: D1PreparedStatement = {
        bind(...next) {
          values = next;
          return statement;
        },
        async run() {
          if (db.failNextRun) {
            db.failNextRun = false;
            throw new Error("D1 unavailable");
          }
          const result = raw.prepare(sql).run(...values);
          return { meta: { last_row_id: Number(result.lastInsertRowid), changes: Number(result.changes) } };
        },
        async all<T>() {
          return { results: raw.prepare(sql).all(...values) as T[] };
        },
        async first<T>() {
          return (raw.prepare(sql).get(...values) as T | undefined) ?? null;
        },
      };
      return statement;
    },
  };
  return db;
}

export interface FakeBucket extends R2Bucket {
  objects: Map<string, Uint8Array>;
  failPut: boolean;
}

export function createBucket(): FakeBucket {
  const bucket: FakeBucket = {
    objects: new Map(),
    failPut: false,
    async put(key, value) {
      if (bucket.failPut) throw new Error("R2 unavailable");
      bucket.objects.set(key, value.slice());
    },
    async get(key) {
      const bytes = bucket.objects.get(key);
      if (bytes === undefined) return null;
      return { size: bytes.byteLength, body: streamOf(bytes) };
    },
    async delete(keys) {
      for (const key of Array.isArray(keys) ? keys : [keys]) bucket.objects.delete(key);
    },
  };
  return bucket;
}

export function streamOf(bytes: Uint8Array, chunkSize = 4096): ReadableStream<Uint8Array> {
  let offset = 0;
  return new ReadableStream<Uint8Array>({
    pull(controller) {
      if (offset >= bytes.byteLength) {
        controller.close();
        return;
      }
      controller.enqueue(bytes.slice(offset, offset + chunkSize));
      offset += chunkSize;
    },
  });
}

export function createEnv(overrides: Partial<Env> = {}): Env & { INBOUND_DB: FakeDb; RAW_MAIL: FakeBucket } {
  return {
    INBOUND_DB: createDb(),
    RAW_MAIL: createBucket(),
    INBOUND_TOKEN: TOKEN,
    ALLOWED_RECIPIENTS: BOT,
    ...overrides,
  } as Env & { INBOUND_DB: FakeDb; RAW_MAIL: FakeBucket };
}

export interface FakeEmail extends InboundEmail {
  rejected: string | null;
}

export function createEmail(
  options: { from?: string; to?: string; raw?: Uint8Array; rawSize?: number } = {},
): FakeEmail {
  const raw = options.raw ?? new TextEncoder().encode(mime());
  const email: FakeEmail = {
    from: options.from ?? "alice@sender.test",
    to: options.to ?? BOT,
    raw: streamOf(raw),
    rawSize: options.rawSize ?? raw.byteLength,
    rejected: null,
    setReject(reason) {
      email.rejected = reason;
    },
  };
  return email;
}

export function mime(headers: string[] = [], body = "hello"): string {
  return [...headers, "From: Alice <alice@sender.test>", "To: bot@example.com", "Subject: hi", "", body, ""].join("\r\n");
}

export const CF_PASS =
  "Authentication-Results: mx.cloudflare.net; dkim=pass header.d=sender.test; dmarc=pass header.from=sender.test policy.dmarc=none; spf=pass (mx.cloudflare.net: domain of alice@sender.test designates 192.0.2.1 as permitted sender) smtp.mailfrom=alice@sender.test";

export function request(path: string, init: RequestInit & { token?: string | null } = {}): Request {
  const { token = TOKEN, ...rest } = init;
  const headers = new Headers(rest.headers);
  if (token !== null) headers.set("authorization", `Bearer ${token}`);
  return new Request(`https://mail.example.com${path}`, { ...rest, headers });
}
