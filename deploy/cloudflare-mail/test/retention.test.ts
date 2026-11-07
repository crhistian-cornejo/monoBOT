import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { handleEmail } from "../src/email.ts";
import { runRetention } from "../src/retention.ts";
import { createEmail, createEnv } from "./support.ts";

const DAY = 24 * 60 * 60 * 1000;
const NOW = 1_800_000_000_000;

async function seedAt(env: ReturnType<typeof createEnv>, ageDays: number[]) {
  for (const age of ageDays) await handleEmail(createEmail(), env, () => NOW - age * DAY);
}

const seqs = (env: ReturnType<typeof createEnv>) =>
  (env.INBOUND_DB.raw.prepare("SELECT seq FROM messages ORDER BY seq").all() as Array<{ seq: number }>).map((r) => r.seq);

describe("retention", () => {
  it("removes expired rows and bodies, keeps fresh ones (default 30 days)", async () => {
    const env = createEnv();
    await seedAt(env, [40, 31, 29, 1]);
    assert.equal(await runRetention(env, () => NOW), 2);
    assert.deepEqual(seqs(env), [3, 4]);
    assert.equal(env.RAW_MAIL.objects.size, 2);
  });

  it("honours RETENTION_DAYS and falls back to 30 for invalid values", async () => {
    const env = createEnv({ RETENTION_DAYS: "7" });
    await seedAt(env, [8, 6]);
    assert.equal(await runRetention(env, () => NOW), 1);
    for (const value of ["0", "-5", "abc", "366", "", "1e2"]) {
      const other = createEnv({ RETENTION_DAYS: value });
      await seedAt(other, [31, 29]);
      assert.equal(await runRetention(other, () => NOW), 1, value);
    }
  });

  it("drains more than one batch", async () => {
    const env = createEnv();
    await seedAt(env, Array.from({ length: 120 }, () => 60));
    assert.equal(await runRetention(env, () => NOW), 120);
    assert.equal(env.RAW_MAIL.objects.size, 0);
  });

  it("retries after a D1 delete failure without orphaning rows", async () => {
    const env = createEnv();
    await seedAt(env, [60, 60]);
    const original = env.INBOUND_DB.prepare.bind(env.INBOUND_DB);
    env.INBOUND_DB.prepare = (sql: string) => {
      if (sql.startsWith("DELETE")) throw new Error("D1 unavailable");
      return original(sql);
    };
    await assert.rejects(runRetention(env, () => NOW));
    assert.deepEqual(seqs(env), [1, 2]);
    env.INBOUND_DB.prepare = original;
    assert.equal(await runRetention(env, () => NOW), 2);
    assert.deepEqual(seqs(env), []);
  });

  it("leaves rows untouched when the R2 delete fails", async () => {
    const env = createEnv();
    await seedAt(env, [60]);
    env.RAW_MAIL.delete = async () => {
      throw new Error("R2 unavailable");
    };
    await assert.rejects(runRetention(env, () => NOW));
    assert.deepEqual(seqs(env), [1]);
  });

  it("does not reuse seq values after retention", async () => {
    const env = createEnv();
    await seedAt(env, [60]);
    await runRetention(env, () => NOW);
    await handleEmail(createEmail(), env, () => NOW);
    assert.deepEqual(seqs(env), [2]);
  });
});
