/**
 * Keeping the same bots on the user's computers through a folder they already
 * sync (OneDrive, iCloud Drive, Dropbox…). Each computer writes only its own
 * file there, so two never write the same one, and reads the others'. A bot is
 * compared three ways: here, on the other computer, and as both last agreed.
 * Pure: no storage, no React.
 */
import { existingMatch, type ExportedBot } from "./botTransfer";

export const SYNC_FORMAT = "monobot.bots.sync";
export const SYNC_VERSION = 1;
export const SYNC_FILE_PREFIX = "monobot-bots-";

export type SyncedBot = ExportedBot & { syncId: string; hash: string };

/** One computer's file in the shared folder. */
export type SyncFile = {
  format: typeof SYNC_FORMAT;
  version: number;
  machine: string;
  /** "Windows" or "Mac", for the user. */
  machineName: string;
  updatedAt: number;
  bots: SyncedBot[];
};

/** Habit fields that change on every run: not part of what a bot is. */
const RUN_FIELDS = [
  "nextRunAt",
  "lastRunAt",
  "lastOutcome",
  "lastError",
  "runs",
  "runRequested",
];

/** The bot as compared and carried: its habits without their run state. */
export function syncContent(bot: ExportedBot): ExportedBot {
  if (!bot.habits) return bot;
  try {
    const habits = JSON.parse(bot.habits) as unknown;
    if (!Array.isArray(habits)) return bot;
    const steady = habits.map((habit) => {
      if (!habit || typeof habit !== "object") return habit;
      const copy = { ...(habit as Record<string, unknown>) };
      for (const field of RUN_FIELDS) delete copy[field];
      return copy;
    });
    return { ...bot, habits: `${JSON.stringify(steady, null, 2)}\n` };
  } catch {
    return bot;
  }
}

/** Object keys in order, so the same bot always hashes the same. */
function stable(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(stable);
  if (!value || typeof value !== "object") return value;
  return Object.fromEntries(
    Object.keys(value as Record<string, unknown>)
      .sort()
      .map((key) => [key, stable((value as Record<string, unknown>)[key])]),
  );
}

/** A fingerprint of what the bot is (not its identity): two FNV-1a passes. */
export function botHash(bot: ExportedBot): string {
  const { syncId: _identity, ...content } = syncContent(bot);
  const text = JSON.stringify(stable(content));
  let a = 0x811c9dc5;
  let b = 0x01000193 ^ text.length;
  for (let index = 0; index < text.length; index++) {
    const code = text.charCodeAt(index);
    a = Math.imul(a ^ code, 0x01000193) >>> 0;
    b = Math.imul(b ^ code, 0x5bd1e995) >>> 0;
  }
  return `${a.toString(16).padStart(8, "0")}${b.toString(16).padStart(8, "0")}`;
}

export function parseSyncFile(raw: string): SyncFile | undefined {
  try {
    const file = JSON.parse(raw) as Partial<SyncFile>;
    if (
      file?.format !== SYNC_FORMAT ||
      typeof file.version !== "number" ||
      file.version > SYNC_VERSION ||
      typeof file.machine !== "string" ||
      !Array.isArray(file.bots)
    )
      return undefined;
    return {
      format: SYNC_FORMAT,
      version: file.version,
      machine: file.machine,
      machineName: typeof file.machineName === "string" ? file.machineName : "another computer",
      updatedAt: typeof file.updatedAt === "number" ? file.updatedAt : 0,
      bots: file.bots.filter(
        (bot): bot is SyncedBot =>
          !!bot &&
          typeof bot.syncId === "string" &&
          typeof bot.hash === "string" &&
          typeof bot.name === "string",
      ),
    };
  } catch {
    return undefined;
  }
}

/** Each bot as the most recently written of the other computers' files has it. */
export function newestRemote(
  files: readonly SyncFile[],
  machine: string,
): Map<string, { bot: SyncedBot; machineName: string }> {
  const newest = new Map<string, { bot: SyncedBot; machineName: string; at: number }>();
  for (const file of files) {
    if (file.machine === machine) continue;
    for (const bot of file.bots) {
      const known = newest.get(bot.syncId);
      if (!known || file.updatedAt > known.at)
        newest.set(bot.syncId, { bot, machineName: file.machineName, at: file.updatedAt });
    }
  }
  return new Map(
    [...newest].map(([syncId, { bot, machineName }]) => [syncId, { bot, machineName }]),
  );
}

/** A bot here, as the sync sees it. */
export type LocalBot = {
  id: string;
  syncId: string;
  name: string;
  botRole?: ExportedBot["botRole"];
  hash: string;
};

export type SyncPlan = {
  /** New on the other computer: made here. */
  create: SyncedBot[];
  /** Changed only there: this one becomes that one. */
  apply: { localId: string; bot: SyncedBot }[];
  /** Changed on both: the user compares and decides. */
  conflicts: { localId: string; bot: SyncedBot; machineName: string }[];
  /** A bot paired by role or name: its identity becomes the shared one. */
  adopt: { localId: string; syncId: string }[];
  /** What both computers now agree each bot is. */
  agreed: Record<string, string>;
};

/**
 * What to do with each bot. `base` is what both agreed each bot was, by sync
 * id. A bot deleted here, which both had agreed on, is not brought back.
 */
export function planBotSync(
  local: readonly LocalBot[],
  remote: ReadonlyMap<string, { bot: SyncedBot; machineName: string }>,
  base: Readonly<Record<string, string>>,
): SyncPlan {
  const plan: SyncPlan = { create: [], apply: [], conflicts: [], adopt: [], agreed: {} };
  const paired = new Set<string>();
  for (const { bot, machineName } of remote.values()) {
    let here = local.find((entry) => entry.syncId === bot.syncId);
    if (!here) {
      // The first sync of two computers that each made "Research": they are one.
      here = existingMatch(
        bot,
        local.filter((entry) => !paired.has(entry.id) && !remote.has(entry.syncId)),
      );
      if (here) {
        // Both sides pick the smaller id, so they settle on the same one.
        if (bot.syncId < here.syncId) plan.adopt.push({ localId: here.id, syncId: bot.syncId });
      }
    }
    if (!here) {
      if (!base[bot.syncId]) plan.create.push(bot);
      continue;
    }
    paired.add(here.id);
    const syncId = bot.syncId < here.syncId ? bot.syncId : here.syncId;
    const agreed = base[syncId] ?? base[here.syncId] ?? base[bot.syncId];
    if (here.hash === bot.hash) plan.agreed[syncId] = bot.hash;
    else if (agreed === here.hash) plan.apply.push({ localId: here.id, bot: { ...bot, syncId } });
    else if (agreed === bot.hash) continue; // Changed here only: the other computer takes it.
    else plan.conflicts.push({ localId: here.id, bot: { ...bot, syncId }, machineName });
  }
  return plan;
}
