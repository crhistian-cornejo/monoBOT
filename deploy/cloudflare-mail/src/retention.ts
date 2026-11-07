import { retentionDays } from "./config.ts";
import type { Clock, Env } from "./types.ts";

const BATCH = 50;
const MAX_BATCHES_PER_RUN = 40;
const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Deletes messages older than the retention window, oldest first.
 *
 * R2 bodies go before their D1 rows: if the row delete fails the row is picked
 * up again on the next run (R2 deletes are idempotent), whereas the opposite
 * order would orphan bodies permanently.
 */
export async function runRetention(env: Env, now: Clock = Date.now): Promise<number> {
  const cutoff = now() - retentionDays(env.RETENTION_DAYS) * DAY_MS;
  let deleted = 0;
  for (let batch = 0; batch < MAX_BATCHES_PER_RUN; batch++) {
    const { results } = await env.INBOUND_DB.prepare(
      "SELECT seq, r2_key FROM messages WHERE received_at < ? ORDER BY seq ASC LIMIT ?",
    )
      .bind(cutoff, BATCH)
      .all<{ seq: number; r2_key: string }>();
    if (results.length === 0) break;

    await env.RAW_MAIL.delete(results.map((row) => row.r2_key));
    const placeholders = results.map(() => "?").join(",");
    await env.INBOUND_DB.prepare(`DELETE FROM messages WHERE seq IN (${placeholders})`)
      .bind(...results.map((row) => row.seq))
      .run();
    deleted += results.length;
    if (results.length < BATCH) break;
  }
  return deleted;
}
