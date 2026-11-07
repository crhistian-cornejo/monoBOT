// Narrow structural subsets of the Cloudflare runtime types. They keep the
// Worker dependency-free and let tests supply fakes backed by node:sqlite.

export type D1Value = string | number | null;

export interface D1Result {
  meta: { last_row_id?: number; changes?: number };
}

export interface D1PreparedStatement {
  bind(...values: D1Value[]): D1PreparedStatement;
  run(): Promise<D1Result>;
  all<T = Record<string, unknown>>(): Promise<{ results: T[] }>;
  first<T = Record<string, unknown>>(): Promise<T | null>;
}

export interface D1Database {
  prepare(sql: string): D1PreparedStatement;
}

export interface R2ObjectBody {
  body: ReadableStream<Uint8Array>;
  size: number;
}

export interface R2Bucket {
  put(
    key: string,
    value: Uint8Array,
    options?: { httpMetadata?: { contentType?: string } },
  ): Promise<unknown>;
  get(key: string): Promise<R2ObjectBody | null>;
  delete(keys: string | string[]): Promise<void>;
}

export interface Env {
  INBOUND_DB: D1Database;
  RAW_MAIL: R2Bucket;
  /** Secret. Must be at least 32 characters or the HTTP API refuses to serve. */
  INBOUND_TOKEN?: string;
  /** Required. Comma separated exact addresses or `*@domain`. */
  ALLOWED_RECIPIENTS?: string;
  RETENTION_DAYS?: string;
  TRUSTED_AUTHSERV_ID?: string;
}

export interface InboundEmail {
  /** SMTP envelope MAIL FROM. */
  readonly from: string;
  /** SMTP envelope RCPT TO. */
  readonly to: string;
  readonly raw: ReadableStream<Uint8Array>;
  readonly rawSize: number;
  setReject(reason: string): void;
}

export interface ScheduledEventLike {
  readonly scheduledTime: number;
  readonly cron: string;
}

export type Clock = () => number;
