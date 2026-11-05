/**
 * Runs the bots' sync through the user's shared folder: writes this
 * computer's file, reads the others', and applies what botSync.ts decides.
 */
import { listDir, readTextFile, writeTextFile } from "../../../platform/tauri/fs";
import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import { findMono, updateMono } from "../../monos/model/mono";
import {
  SYNC_FILE_PREFIX,
  SYNC_FORMAT,
  SYNC_VERSION,
  botHash,
  newestRemote,
  parseSyncFile,
  planBotSync,
  syncContent,
  type SyncFile,
  type SyncedBot,
} from "./botSync";
import {
  createImportedBot,
  exportableBots,
  replaceBot,
  snapshotBot,
} from "./botTransferIO";

const FOLDER_KEY = "monocode.botSyncFolder";
const MACHINE_KEY = "monocode.machineId";
const BASE_KEY = "monocode.botSyncBase";
const LAST_KEY = "monocode.botSyncLast";
const CHANGED = "monocode:bot-sync-changed";

const read = (key: string) => {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
};
const write = (key: string, value: string | null) => {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // private mode / quota
  }
  window.dispatchEvent(new Event(CHANGED));
};

export function loadSyncFolder(): string | undefined {
  return read(FOLDER_KEY) ?? undefined;
}

export function saveSyncFolder(folder: string | undefined): void {
  write(FOLDER_KEY, folder ?? null);
}

const SYNC_NOW = "monocode:bot-sync-now";

/** Asks the running sync to go now ("Sync now" in Settings). */
export function requestBotSync(): void {
  window.dispatchEvent(new Event(SYNC_NOW));
}

export function subscribeBotSyncRequests(onRequest: () => void): () => void {
  window.addEventListener(SYNC_NOW, onRequest);
  return () => window.removeEventListener(SYNC_NOW, onRequest);
}

export function subscribeBotSync(onChange: () => void): () => void {
  window.addEventListener(CHANGED, onChange);
  return () => window.removeEventListener(CHANGED, onChange);
}

/** This computer, in the shared folder. */
function machineId(): string {
  const known = read(MACHINE_KEY);
  if (known) return known;
  const id = crypto.randomUUID().slice(0, 8);
  write(MACHINE_KEY, id);
  return id;
}

const machineName = IS_MAC ? "Mac" : IS_WIN ? "Windows" : "Linux";

function loadBase(): Record<string, string> {
  try {
    return JSON.parse(read(BASE_KEY) ?? "{}") as Record<string, string>;
  } catch {
    return {};
  }
}

function saveBase(base: Record<string, string>): void {
  write(BASE_KEY, JSON.stringify(base));
}

/** The user chose for a bot both had changed: that version is now agreed. */
export function settleBot(syncId: string, hash: string): void {
  saveBase({ ...loadBase(), [syncId]: hash });
}

export type SyncStatus = { at: number; text: string; error?: boolean };

export function loadSyncStatus(): SyncStatus | undefined {
  try {
    return JSON.parse(read(LAST_KEY) ?? "null") ?? undefined;
  } catch {
    return undefined;
  }
}

export type SyncConflict = { localId: string; bot: SyncedBot; machineName: string };

const join = (folder: string, name: string) =>
  `${folder.replace(/[\\/]+$/, "")}${folder.includes("\\") ? "\\" : "/"}${name}`;

async function writeOwnFile(folder: string, machine: string): Promise<SyncedBot[]> {
  const bots = await Promise.all(
    exportableBots().map(async (mono) => {
      const bot = syncContent(await snapshotBot(mono));
      return { ...bot, syncId: bot.syncId!, hash: botHash(bot) };
    }),
  );
  const path = join(folder, `${SYNC_FILE_PREFIX}${machine}.json`);
  // Unchanged bots leave the file alone: the cloud folder has nothing to upload.
  const before = await readTextFile(path)
    .then(parseSyncFile)
    .catch(() => undefined);
  const fingerprint = (list: readonly SyncedBot[]) =>
    list
      .map((bot) => `${bot.syncId}:${bot.hash}`)
      .sort()
      .join(",");
  if (before && fingerprint(before.bots) === fingerprint(bots)) return bots;
  const file: SyncFile = {
    format: SYNC_FORMAT,
    version: SYNC_VERSION,
    machine,
    machineName,
    updatedAt: Date.now(),
    bots,
  };
  await writeTextFile(path, `${JSON.stringify(file, null, 2)}\n`);
  return bots;
}

async function readOtherFiles(folder: string, machine: string): Promise<SyncFile[]> {
  const entries = await listDir(folder);
  const files = await Promise.all(
    entries
      .filter(
        (entry) =>
          !entry.isDir &&
          entry.name.startsWith(SYNC_FILE_PREFIX) &&
          entry.name.endsWith(".json") &&
          entry.name !== `${SYNC_FILE_PREFIX}${machine}.json`,
      )
      .map((entry) =>
        readTextFile(entry.path)
          .then(parseSyncFile)
          .catch(() => undefined),
      ),
  );
  return files.filter((file): file is SyncFile => !!file);
}

/**
 * One sync: writes this computer's bots, takes what changed on the others,
 * and returns the bots both changed, for the user to decide.
 */
export async function runBotSync(): Promise<SyncConflict[]> {
  const folder = loadSyncFolder();
  if (!folder) return [];
  const machine = machineId();
  try {
    const mine = await writeOwnFile(folder, machine);
    const others = await readOtherFiles(folder, machine);
    const remote = newestRemote(others, machine);
    const base = loadBase();
    const local = exportableBots().flatMap((mono) => {
      const bot = mine.find((entry) => entry.syncId === mono.syncId);
      return bot
        ? [{ id: mono.id, syncId: bot.syncId, name: bot.name, botRole: bot.botRole, hash: bot.hash }]
        : [];
    });
    const plan = planBotSync(local, remote, base);
    for (const { localId, syncId } of plan.adopt)
      updateMono(localId, (mono) => ({ ...mono, syncId }));
    for (const { localId, bot } of plan.apply) {
      if (!findMono(localId)) continue;
      await replaceBot(localId, bot);
      plan.agreed[bot.syncId] = bot.hash;
    }
    for (const bot of plan.create) {
      await createImportedBot(bot);
      plan.agreed[bot.syncId] = bot.hash;
    }
    saveBase({ ...base, ...plan.agreed });
    // What was taken from the others goes out again with this computer's file.
    if (plan.apply.length || plan.create.length || plan.adopt.length)
      await writeOwnFile(folder, machine);
    const others_ = [...new Set(others.map((file) => file.machineName))];
    const changed = plan.apply.length + plan.create.length;
    write(
      LAST_KEY,
      JSON.stringify({
        at: Date.now(),
        text: others_.length
          ? `Synced with ${others_.join(", ")}${changed ? `: ${changed} bot${changed === 1 ? "" : "s"} updated` : ""}`
          : "Waiting for your other computer to sync to this folder",
      } satisfies SyncStatus),
    );
    return plan.conflicts;
  } catch (failure) {
    write(
      LAST_KEY,
      JSON.stringify({
        at: Date.now(),
        text: failure instanceof Error ? failure.message : String(failure),
        error: true,
      } satisfies SyncStatus),
    );
    return [];
  }
}
