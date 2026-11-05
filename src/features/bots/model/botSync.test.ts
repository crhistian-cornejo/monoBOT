import { describe, expect, it } from "vitest";
import type { ExportedBot } from "./botTransfer";
import {
  botHash,
  newestRemote,
  parseSyncFile,
  planBotSync,
  syncContent,
  type LocalBot,
  type SyncFile,
  type SyncedBot,
} from "./botSync";

const bot = (patch: Partial<ExportedBot> = {}): ExportedBot => ({
  syncId: "bbbbbbbb-1",
  name: "Research",
  mascot: "ghost",
  color: "#7c3aed",
  botRole: "research",
  soul: "# Research",
  memory: "- Mining",
  topics: {},
  ...patch,
});

const synced = (patch: Partial<ExportedBot> = {}): SyncedBot => {
  const content = bot(patch);
  return { ...content, syncId: content.syncId!, hash: botHash(content) };
};

const local = (patch: Partial<ExportedBot> = {}, id = "here"): LocalBot => {
  const content = bot(patch);
  return {
    id,
    syncId: content.syncId!,
    name: content.name,
    botRole: content.botRole,
    hash: botHash(content),
  };
};

const remote = (...bots: SyncedBot[]) =>
  new Map(bots.map((entry) => [entry.syncId, { bot: entry, machineName: "Mac" }]));

describe("what a bot is", () => {
  it("ignores a habit's run state, and the bot's identity", () => {
    const habit = { id: "h", name: "Papers", instructions: "x", schedule: {} };
    const before = bot({ habits: JSON.stringify([{ ...habit, nextRunAt: 1, runs: [] }]) });
    const after = bot({
      syncId: "zzzzzzzz-9",
      habits: JSON.stringify([{ ...habit, nextRunAt: 99, lastRunAt: 50, lastOutcome: "posted" }]),
    });
    expect(botHash(before)).toBe(botHash(after));
    expect(syncContent(after).habits).not.toContain("lastRunAt");
    expect(botHash(bot({ memory: "- Mining\n- Lima" }))).not.toBe(botHash(before));
  });

  it("reads the other computers' files, newest first", () => {
    const file = (machine: string, updatedAt: number, memory: string): SyncFile => ({
      format: "monobot.bots.sync",
      version: 1,
      machine,
      machineName: machine,
      updatedAt,
      bots: [synced({ memory })],
    });
    const parsed = parseSyncFile(JSON.stringify(file("mac", 5, "new")))!;
    expect(parsed.bots[0].memory).toBe("new");
    expect(parseSyncFile("{}")).toBeUndefined();
    const newest = newestRemote([file("me", 9, "mine"), file("old", 1, "old"), parsed], "me");
    expect(newest.get("bbbbbbbb-1")?.bot.memory).toBe("new");
  });
});

describe("planBotSync", () => {
  it("agrees when both are the same", () => {
    const plan = planBotSync([local()], remote(synced()), {});
    expect(plan.agreed).toEqual({ "bbbbbbbb-1": synced().hash });
    expect(plan.apply).toEqual([]);
  });

  it("takes the other's change when only it changed", () => {
    const plan = planBotSync(
      [local()],
      remote(synced({ memory: "- Mining\n- Lima" })),
      { "bbbbbbbb-1": local().hash },
    );
    expect(plan.apply).toHaveLength(1);
    expect(plan.conflicts).toEqual([]);
  });

  it("keeps this one's change for the other to take", () => {
    const here = local({ memory: "- changed here" });
    const plan = planBotSync([here], remote(synced()), { "bbbbbbbb-1": synced().hash });
    expect(plan.apply).toEqual([]);
    expect(plan.conflicts).toEqual([]);
  });

  it("asks the user when both changed", () => {
    const plan = planBotSync(
      [local({ memory: "- here" })],
      remote(synced({ memory: "- there" })),
      { "bbbbbbbb-1": synced().hash },
    );
    expect(plan.conflicts).toEqual([
      expect.objectContaining({ localId: "here", machineName: "Mac" }),
    ]);
  });

  it("creates a new bot, but not one deleted here", () => {
    const writer = synced({ syncId: "wwwwwwww-1", name: "Writer", botRole: "writer" });
    expect(planBotSync([], remote(writer), {}).create).toEqual([writer]);
    expect(planBotSync([], remote(writer), { "wwwwwwww-1": writer.hash }).create).toEqual([]);
  });

  it("pairs the two computers' own Research and settles on one id", () => {
    const here = local({ syncId: "zzzzzzzz-9" });
    const there = synced({ syncId: "aaaaaaaa-1" });
    const plan = planBotSync([here], remote(there), {});
    expect(plan.adopt).toEqual([{ localId: "here", syncId: "aaaaaaaa-1" }]);
    expect(plan.agreed).toEqual({ "aaaaaaaa-1": there.hash });
    // The other computer keeps its smaller id.
    const back = planBotSync([local({ syncId: "aaaaaaaa-1" })], remote(synced({ syncId: "zzzzzzzz-9" })), {});
    expect(back.adopt).toEqual([]);
  });
});
