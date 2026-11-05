// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const files = new Map<string, string>();
vi.mock("../../monos/model/monoFiles", () => ({
  loadMonoFiles: vi.fn(async (id: string) => ({
    id,
    dir: `/agents/${id}`,
    soul: files.get(`${id}/SOUL.md`) ?? "",
    soulHash: "s",
    memory: files.get(`${id}/MEMORY.md`) ?? "",
    memoryHash: "m",
    memoryPath: `/agents/${id}/MEMORY.md`,
    topics: [...files.keys()]
      .filter((key) => key.startsWith(`${id}/memory/`))
      .map((key) => key.slice(`${id}/memory/`.length, -3)),
  })),
  readAgentFile: vi.fn(async (id: string, path: string) => ({
    text: files.get(`${id}/${path}`) ?? null,
    hash: "h",
  })),
  writeAgentFile: vi.fn(async (id: string, path: string, text: string) => {
    files.set(`${id}/${path}`, text);
    return "h";
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn(), save: vi.fn() }));

const { createMono, findMono, listMonos, updateMono } = await import(
  "../../monos/model/mono"
);
const { createImportedBot, replaceBot, snapshotBot } = await import(
  "./botTransferIO"
);

const incoming = {
  name: "Research",
  mascot: "ghost",
  color: "#7c3aed",
  botRole: "research" as const,
  specialty: "Reports with sources",
  botTools: { services: [], plugins: [], connectors: ["exa"] },
  soul: "# Research from the Mac",
  memory: "- Learned on the Mac",
  topics: { mining: "- Pozas de sedimentación" },
  habits: '[{"id":"h1","name":"Weekly papers"}]',
};

beforeEach(() => {
  localStorage.clear();
  files.clear();
});

describe("importing bots", () => {
  it("replaces a bot but keeps its chat and this computer's folders", async () => {
    const here = createMono([], { role: "research", name: "Research", instructions: "" });
    updateMono(here.id, (mono) => ({
      ...mono,
      sessionId: "chat-1",
      botTools: {
        services: [],
        plugins: [],
        folders: [{ path: "C:\\Users\\me\\Docs", edit: false }],
      },
    }));
    await replaceBot(here.id, incoming);
    const after = findMono(here.id)!;
    expect(after).toMatchObject({
      sessionId: "chat-1",
      specialty: "Reports with sources",
      botTools: {
        connectors: ["exa"],
        folders: [{ path: "C:\\Users\\me\\Docs", edit: false }],
      },
    });
    expect(files.get(`${here.id}/SOUL.md`)).toBe("# Research from the Mac");
    expect(files.get(`${here.id}/memory/mining.md`)).toBe("- Pozas de sedimentación");
    expect(files.get(`${here.id}/habits.json`)).toContain("Weekly papers");
  });

  it("keeps both under a name that isn't taken, and exports what it imported", async () => {
    createMono([], { role: "research", name: "Research", instructions: "" });
    const copy = await createImportedBot(incoming, true);
    expect(copy.name).toBe("Research (2)");
    expect(listMonos()).toHaveLength(2);
    const exported = await snapshotBot(findMono(copy.id)!);
    expect(exported).toMatchObject({
      name: "Research (2)",
      mascot: "ghost",
      soul: "# Research from the Mac",
      topics: { mining: "- Pozas de sedimentación" },
      botTools: { connectors: ["exa"] },
    });
  });
});
