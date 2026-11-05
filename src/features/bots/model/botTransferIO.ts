/**
 * Reads and writes real bots for an export or an import: the roster's card and
 * the agent folder's files. The decisions themselves are in botTransfer.ts.
 */
import { open, save } from "@tauri-apps/plugin-dialog";
import { readTextFile, writeTextFile } from "../../../platform/tauri/fs";
import { IS_MAC, IS_WIN } from "../../../platform/tauri/platform";
import {
  createMono,
  listMonos,
  monoLook,
  updateMono,
  type Mono,
} from "../../monos/model/mono";
import {
  loadMonoFiles,
  readAgentFile,
  writeAgentFile,
} from "../../monos/model/monoFiles";
import { readSession } from "../../sessions/data/sessionStore";
import {
  botExport,
  freeName,
  parseBotModel,
  type BotModel,
  parseBotExport,
  portableTools,
  type BotExport,
  type ExportedBot,
} from "./botTransfer";

/** The bots an export carries: the user's assistants, not coding Monos or groups. */
export function exportableBots(): Mono[] {
  return listMonos().filter((mono) => mono.kind === "bot");
}

/** The bot's identity across computers, given the first time it is needed. */
export function ensureSyncId(mono: Mono): string {
  if (mono.syncId) return mono.syncId;
  const syncId = crypto.randomUUID();
  updateMono(mono.id, (current) => ({ ...current, syncId }));
  return syncId;
}

/** A bot as it would travel: its card, SOUL.md, memory and habits. */
export async function snapshotBot(mono: Mono): Promise<ExportedBot> {
  const syncId = ensureSyncId(mono);
  const look = monoLook(mono);
  const files = await loadMonoFiles(mono.id);
  const topics: Record<string, string> = {};
  for (const topic of files.topics) {
    const file = await readAgentFile(mono.id, `memory/${topic}.md`).catch(
      () => undefined,
    );
    if (file?.text != null) topics[topic] = file.text;
  }
  const habits = await readAgentFile(mono.id, "habits.json").catch(
    () => undefined,
  );
  const model = mono.modelChoice ?? (await sessionModel(mono));
  const tools = portableTools(mono.botTools);
  return {
    syncId,
    name: look.name,
    mascot: mono.mascot,
    color: mono.color,
    ...(mono.botRole ? { botRole: mono.botRole } : {}),
    ...(mono.specialty ? { specialty: mono.specialty } : {}),
    ...(tools ? { botTools: tools } : {}),
    soul: files.soul,
    memory: files.memory,
    topics,
    ...(habits?.text ? { habits: habits.text } : {}),
    ...(model ? { model } : {}),
  };
}

/** The model and permissions of the bot's chat, as last saved. */
async function sessionModel(mono: Mono): Promise<BotModel | undefined> {
  if (!mono.sessionId) return undefined;
  const session = await readSession(mono.sessionId).catch(() => null);
  return session
    ? parseBotModel({
        harness: session.harness,
        model: session.model,
        modelSettings: session.modelSettings,
        runtimeMode: session.runtimeMode,
      })
    : undefined;
}

const platform = IS_MAC ? "macos" : IS_WIN ? "windows" : "linux";

/** Asks where to save, then writes every bot. Null when the user cancels. */
export async function exportBotsToFile(): Promise<{
  path: string;
  count: number;
} | null> {
  const bots = exportableBots();
  if (!bots.length) throw new Error("There are no bots to export yet.");
  const day = new Date().toISOString().slice(0, 10);
  const path = await save({
    title: "Export bots",
    defaultPath: `monobot-bots-${day}.json`,
    filters: [{ name: "MonoBOT bots", extensions: ["json"] }],
  });
  if (!path) return null;
  const snapshots = await Promise.all(bots.map(snapshotBot));
  await writeTextFile(
    path,
    `${JSON.stringify(botExport(snapshots, platform), null, 2)}\n`,
  );
  return { path, count: snapshots.length };
}

/** Asks for an export file and reads it. Null when the user cancels. */
export async function pickBotExport(): Promise<BotExport | null> {
  const path = await open({
    title: "Import bots",
    multiple: false,
    directory: false,
    filters: [{ name: "MonoBOT bots", extensions: ["json"] }],
  });
  if (!path || Array.isArray(path)) return null;
  return parseBotExport(await readTextFile(path));
}

/** Writes the incoming bot's files over its folder: SOUL, memory, topics, habits. */
async function writeFiles(monoId: string, bot: ExportedBot): Promise<void> {
  await writeAgentFile(monoId, "SOUL.md", bot.soul);
  await writeAgentFile(monoId, "MEMORY.md", bot.memory);
  for (const [topic, body] of Object.entries(bot.topics))
    await writeAgentFile(monoId, `memory/${topic}.md`, body);
  if (bot.habits) await writeAgentFile(monoId, "habits.json", bot.habits);
}

/** A new bot from the file; with `copy`, under a name that isn't taken. */
export async function createImportedBot(
  bot: ExportedBot,
  copy = false,
): Promise<Mono> {
  const name = copy
    ? freeName(
        bot.name,
        listMonos().map((mono) => monoLook(mono).name),
      )
    : bot.name;
  const mono = createMono([], {
    role: bot.botRole ?? "custom",
    name,
    instructions: "",
  });
  await writeFiles(mono.id, bot);
  return (
    updateMono(mono.id, (current) => ({
      ...current,
      // A copy kept beside the original is a bot of its own.
      ...(bot.syncId && !copy ? { syncId: bot.syncId } : {}),
      mascot: bot.mascot,
      color: bot.color,
      ...(bot.specialty ? { specialty: bot.specialty } : {}),
      ...(bot.botTools ? { botTools: { ...bot.botTools } } : {}),
      ...(bot.model ? { modelChoice: bot.model } : {}),
    })) ?? mono
  );
}

/**
 * The bot here becomes the incoming one: card, instructions, memory, habits.
 * Its conversation and the folders opened to it on this computer stay.
 */
export async function replaceBot(
  existingId: string,
  bot: ExportedBot,
): Promise<void> {
  await writeFiles(existingId, bot);
  updateMono(existingId, (current) => ({
    ...current,
    ...(bot.syncId ? { syncId: bot.syncId } : {}),
    name: bot.name,
    mascot: bot.mascot,
    color: bot.color,
    specialty: bot.specialty ?? "",
    ...(bot.model ? { modelChoice: bot.model } : {}),
    ...(bot.botTools
      ? {
          botTools: {
            ...bot.botTools,
            ...(current.botTools?.folders
              ? { folders: current.botTools.folders }
              : {}),
          },
        }
      : {}),
  }));
}
