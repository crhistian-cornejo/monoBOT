// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const agentFiles = new Map<string, string>();
const disk = new Map<string, string>();

vi.mock("../../monos/model/monoFiles", () => ({
  loadMonoFiles: vi.fn(async (id: string) => ({
    id,
    dir: `/agents/${id}`,
    soul: agentFiles.get(`${id}/SOUL.md`) ?? "",
    soulHash: "s",
    memory: agentFiles.get(`${id}/MEMORY.md`) ?? "",
    memoryHash: "m",
    memoryPath: "",
    topics: [],
  })),
  readAgentFile: vi.fn(async (id: string, path: string) => ({
    text: agentFiles.get(`${id}/${path}`) ?? null,
    hash: "h",
  })),
  writeAgentFile: vi.fn(async (id: string, path: string, text: string) => {
    agentFiles.set(`${id}/${path}`, text);
    return "h";
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock("../../../platform/tauri/fs", () => ({
  listDir: vi.fn(async (folder: string) =>
    [...disk.keys()]
      .filter((path) => path.startsWith(`${folder}/`))
      .map((path) => ({ name: path.slice(folder.length + 1), path, isDir: false, ignored: false })),
  ),
  readTextFile: vi.fn(async (path: string) => {
    const text = disk.get(path);
    if (text == null) throw new Error("missing");
    return text;
  }),
  writeTextFile: vi.fn(async (path: string, text: string) => {
    disk.set(path, text);
  }),
}));

const { createMono, findMono, listMonos, updateMono } = await import("../../monos/model/mono");
const { botHash } = await import("./botSync");
const { runBotSync, saveSyncFolder, settleBot } = await import("./botSyncIO");
const { snapshotBot } = await import("./botTransferIO");
const { writeTextFile } = await import("../../../platform/tauri/fs");

const FOLDER = "/OneDrive/MonoBOT";

/** The Mac's file in the shared folder, with the given bots. */
function macFile(bots: object[], updatedAt = Date.now()) {
  disk.set(
    `${FOLDER}/monobot-bots-mac00001.json`,
    JSON.stringify({ format: "monobot.bots.sync", version: 1, machine: "mac00001", machineName: "Mac", updatedAt, bots }),
  );
}

async function researchHere(memory = "- Mining") {
  const mono = createMono([], { role: "research", name: "Research", instructions: "" });
  updateMono(mono.id, (current) => ({ ...current, syncId: "aaaaaaaa-1" }));
  agentFiles.set(`${mono.id}/SOUL.md`, "# Research");
  agentFiles.set(`${mono.id}/MEMORY.md`, memory);
  return findMono(mono.id)!;
}

async function asMac(mono: ReturnType<typeof findMono>, patch: object) {
  const bot = { ...(await snapshotBot(mono!)), ...patch };
  return { ...bot, hash: botHash(bot) };
}

beforeEach(() => {
  localStorage.clear();
  agentFiles.clear();
  disk.clear();
  vi.mocked(writeTextFile).mockClear();
  saveSyncFolder(FOLDER);
});

describe("syncing bots through a shared folder", () => {
  it("writes this computer's file, and only again when a bot changed", async () => {
    await researchHere();
    expect(await runBotSync()).toEqual([]);
    const own = [...disk.keys()].find((path) => path !== `${FOLDER}/monobot-bots-mac00001.json`)!;
    expect(JSON.parse(disk.get(own)!).bots[0]).toMatchObject({ name: "Research", syncId: "aaaaaaaa-1" });
    vi.mocked(writeTextFile).mockClear();
    await runBotSync();
    expect(writeTextFile).not.toHaveBeenCalled();
  });

  it("takes a bot that changed only on the Mac, and creates a new one", async () => {
    const here = await researchHere();
    const agreed = await asMac(here, {});
    settleBot("aaaaaaaa-1", agreed.hash);
    const writer = { ...agreed, syncId: "wwwwwwww-1", name: "Writer", botRole: "writer", memory: "", soul: "# Writer" };
    macFile([await asMac(here, { memory: "- Mining\n- Lima" }), { ...writer, hash: botHash(writer) }]);
    expect(await runBotSync()).toEqual([]);
    expect(agentFiles.get(`${here.id}/MEMORY.md`)).toBe("- Mining\n- Lima");
    expect(listMonos().map((mono) => mono.name)).toEqual(["Research", "Writer"]);
  });

  it("asks when the bot changed on both computers", async () => {
    const here = await researchHere();
    settleBot("aaaaaaaa-1", (await asMac(here, {})).hash);
    agentFiles.set(`${here.id}/MEMORY.md`, "- changed on Windows");
    macFile([await asMac(here, { memory: "- changed on the Mac" })]);
    const conflicts = await runBotSync();
    expect(conflicts).toEqual([
      expect.objectContaining({ localId: here.id, machineName: "Mac" }),
    ]);
    expect(agentFiles.get(`${here.id}/MEMORY.md`)).toBe("- changed on Windows");
  });
});
