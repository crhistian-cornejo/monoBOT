// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { botLaunchForSession, groupLaunchForSession } from "./botSession";

const stored = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => stored.clear(),
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => stored.set(key, value),
  removeItem: (key: string) => stored.delete(key),
});

const base = { mascot: "crab", color: "red", projects: [] };

beforeEach(() => {
  stored.clear();
  stored.set("monocode:principal-tools-reset", "1");
  stored.set(
    "monocode:mono-roster",
    JSON.stringify([
      {
        ...base,
        id: "docs",
        sessionId: "docs-chat",
        kind: "bot",
        botRole: "documents",
      },
      {
        ...base,
        id: "finance",
        sessionId: "finance-chat",
        kind: "bot",
        botRole: "finance",
      },
      { ...base, id: "coder", sessionId: "coder-chat", projects: ["/src/app"] },
      {
        ...base,
        id: "nexa",
        sessionId: "nexa-chat",
        kind: "group",
        group: {
          members: ["docs", "finance", "coder"],
          folders: [
            { path: "D:\\Projects\\1064", edit: false },
            { path: "D:\\Projects\\1064\\Editables", edit: true },
          ],
          context: ["D:\\Briefs\\obs.pdf"],
        },
      },
    ]),
  );
});

describe("a group's folders in its members' sessions", () => {
  it("opens them to a bot at the group's access, read only enforced", () => {
    const launch = botLaunchForSession("docs-chat")!;
    expect(launch.addDirs).toEqual([
      "D:\\Projects\\1064",
      "D:\\Projects\\1064\\Editables",
    ]);
    expect(launch.allowedTools).toContain("Read(//d/Projects/1064/**)");
    expect(launch.allowedTools).toContain("Edit(//d/Projects/1064/Editables/**)");
    expect(launch.allowedTools).toContain("Read(//d/Briefs/obs.pdf)");
    expect(launch.disallowedTools).toContain("Edit(//d/Projects/1064/**)");
    expect(launch.disallowedTools).not.toContain(
      "Edit(//d/Projects/1064/Editables/**)",
    );
  });

  it("keeps a bot that may not change files from changing them", () => {
    const launch = botLaunchForSession("finance-chat")!;
    expect(launch.addDirs).toContain("D:\\Projects\\1064\\Editables");
    expect(launch.allowedTools).not.toContain(
      "Edit(//d/Projects/1064/Editables/**)",
    );
    expect(launch.disallowedTools).toContain("Edit");
  });

  it("gives the group chat its folders and keeps subagents to the members", () => {
    const launch = groupLaunchForSession("nexa-chat")!;
    expect(launch.addDirs).toHaveLength(2);
    expect(launch.disallowedTools).toEqual(
      expect.arrayContaining(["Task", "Agent", "Write(//d/Projects/1064/**)"]),
    );
    expect(groupLaunchForSession("coder-chat")?.allowedTools).toContain(
      "Write(//d/Projects/1064/Editables/**)",
    );
    expect(groupLaunchForSession("docs-chat")).toBeNull();
    expect(groupLaunchForSession("nobody")).toBeNull();
  });
});
