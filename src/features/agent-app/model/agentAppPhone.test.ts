// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { findMono, listMonos } from "../../monos/model/mono";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

vi.mock("../../../integrations/harness/core/availability", () => ({
  isHarnessAvailable: () => true,
}));

const stored = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => stored.clear(),
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => stored.set(key, value),
  removeItem: (key: string) => stored.delete(key),
});

const bot = (id: string, botRole: string) => ({
  id,
  sessionId: `${id}-chat`,
  name: id,
  mascot: "crab",
  color: "red",
  projects: [],
  kind: "bot",
  botRole,
});

beforeEach(() => {
  stored.clear();
  stored.set(
    "monocode:mono-roster",
    JSON.stringify([
      bot("ivy", "principal"),
      bot("research", "research"),
      bot("phone", "phone"),
    ]),
  );
  stored.set("monocode:principal-tools-reset", "1");
});

const host = {
  installedSkills: async () => [],
} as unknown as AgentAppHost;

const call = (from: string, action: string, input: Record<string, unknown>) =>
  handleAgentApp({ id: `${from}-chat` } as never, "r1", action, input, host);

describe("the iPhone connector is the user's to grant", () => {
  it("refuses a bot that adds it to its own tools", async () => {
    await expect(
      call("research", "tools.update", { connectors: ["xmd", "poki"] }),
    ).rejects.toThrow(/Only the user can give a bot iPhone \(Poki\)/);
    expect(findMono("research")?.botTools?.connectors ?? []).not.toContain(
      "poki",
    );
  });

  it("lets a Phone bot keep it, or drop it", async () => {
    await call("phone", "tools.update", { connectors: ["poki"] });
    expect(findMono("phone")?.botTools?.connectors).toEqual(["poki"]);
    await call("phone", "tools.update", { connectors: [] });
    expect(findMono("phone")?.botTools?.connectors).toEqual([]);
    // Once dropped, the bot cannot take it back.
    await expect(
      call("phone", "tools.update", { connectors: ["poki"] }),
    ).rejects.toThrow(/Only the user/);
  });

  it("keeps the Principal from creating a Phone bot or handing it out", async () => {
    const before = listMonos().length;
    await expect(
      call("ivy", "bots.create", { name: "Pocket", role: "phone" }),
    ).rejects.toThrow(/only the user creates one/);
    await expect(
      call("ivy", "bots.create", {
        name: "Scout",
        role: "research",
        connectors: ["exa", "poki"],
      }),
    ).rejects.toThrow(/Only the user/);
    expect(listMonos()).toHaveLength(before);
  });
});
