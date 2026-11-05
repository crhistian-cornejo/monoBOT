import { describe, expect, it } from "vitest";
import type { Mono } from "../../monos/model/mono";
import type { Block } from "../../sessions/model/session";
import type { Delegation } from "../../bots/model/delegations";
import { groupActivity, groupExchanges, latestRuns } from "./groupActivity";
import { memorySeed } from "./groupProject";

const run = (id: string): Block => ({
  id,
  role: "assistant",
  text: "",
  agentRun: { name: id, steps: [] },
});

const monos: Mono[] = [
  {
    id: "ivy",
    sessionId: "ivy-chat",
    name: "IVY",
    mascot: "crab",
    color: "red",
    projects: [],
    kind: "bot",
    botRole: "principal",
  },
  {
    id: "research",
    sessionId: "research-chat",
    name: "Research",
    mascot: "ghost",
    color: "blue",
    projects: [],
    kind: "bot",
    botRole: "research",
  },
  {
    id: "docs",
    sessionId: "docs-chat",
    name: "Documents",
    mascot: "invader",
    color: "green",
    projects: [],
    kind: "bot",
    botRole: "documents",
  },
];
const group: Mono = {
  id: "nexa",
  sessionId: "nexa-chat",
  name: "Nexa",
  mascot: "mushroom",
  color: "purple",
  projects: [],
  kind: "group",
  group: {
    members: ["research", "ivy", "docs"],
    seats: { research: "research-seat" },
  },
};
const find = (id: string) => monos.find((mono) => mono.id === id);

describe("a group's activity", () => {
  it("keeps only the latest turn's subagents", () => {
    const blocks: Block[] = [
      { id: "u1", role: "user", text: "first" },
      run("old"),
      { id: "u2", role: "user", text: "second" },
      { id: "i", role: "user", text: "keep going", internal: true },
      run("a"),
      { id: "t", role: "assistant", text: "Working" },
      run("b"),
    ];
    expect(latestRuns(blocks).map((block) => block.id)).toEqual(["a", "b"]);
  });

  it("lists the Principal first, from the group chat, then each member's seat", () => {
    const sessions = [
      {
        id: "nexa-chat",
        cwd: "/home",
        blocks: [{ id: "u", role: "user", text: "go" } as Block],
        busy: true,
      },
      {
        id: "research-seat",
        cwd: "/home",
        harness: "claude" as const,
        model: "sonnet",
        blocks: [
          {
            id: "u",
            role: "user",
            text: "find",
            turnMetrics: { inputTokens: 1200, outputTokens: 300 },
          } as Block,
          run("papers"),
          run("web"),
        ],
        busy: true,
      },
      // Its own chat is busy with something else: not the group's work.
      {
        id: "docs-chat",
        cwd: "/home",
        blocks: [{ id: "u", role: "user", text: "mine" } as Block],
        busy: true,
      },
    ];
    const rows = groupActivity(
      group,
      sessions,
      new Map([["docs", { status: "needs-you", activity: "Approve Write" }]]),
      find,
    );
    expect(rows.map((row) => [row.name, row.lead, row.state.status])).toEqual([
      ["IVY", true, "working"],
      ["Research", false, "working"],
      ["Documents", false, "idle"],
    ]);
    expect(rows[1]!.runs.map((block) => block.id)).toEqual(["papers", "web"]);
    expect(rows[1]!.cwd).toBe("/home");
    expect(rows[1]!.usage).toMatchObject({ turns: 1, tokens: 1500 });
    expect(rows[2]!.runs).toEqual([]);
    expect(rows[2]!.usage).toBeUndefined();
  });

  it("times a working member's turn, and gives the lead its usage too", () => {
    const rows = groupActivity(
      group,
      [
        {
          id: "nexa-chat",
          cwd: "/home",
          harness: "claude" as const,
          model: "sonnet",
          blocks: [
            {
              id: "u",
              role: "user",
              text: "go",
              startedAt: 5_000,
              turnMetrics: { inputTokens: 100, outputTokens: 20 },
            } as Block,
          ],
          busy: false,
        },
        {
          id: "research-seat",
          cwd: "/home",
          harness: "claude" as const,
          model: "sonnet",
          blocks: [
            { id: "u1", role: "user", text: "first", startedAt: 1_000 } as Block,
            { id: "u2", role: "user", text: "find", startedAt: 9_000 } as Block,
            {
              id: "i",
              role: "user",
              text: "keep going",
              internal: true,
              startedAt: 12_000,
            } as Block,
          ],
          busy: true,
        },
      ],
      new Map(),
      find,
    );
    const [lead, research] = rows;
    expect(lead!.state.status).toBe("idle");
    expect(lead!.startedAt).toBeUndefined();
    expect(lead!.usage).toMatchObject({ turns: 1, tokens: 120 });
    expect(research!.state.status).toBe("working");
    expect(research!.startedAt).toBe(9_000);
  });

  it("shows what members asked each other lately", () => {
    const now = 1_000_000_000;
    const entry = (id: string, from: string, to: string, at: number) =>
      ({ id, from, to, message: id, at }) as Delegation;
    const exchanges = groupExchanges(
      group,
      [
        entry("1", "research", "docs", now - 1000),
        entry("2", "ivy", "outsider", now - 1000),
        entry("3", "docs", "research", now - 2 * 24 * 60 * 60 * 1000),
      ],
      now,
    );
    expect(exchanges.map((item) => item.id)).toEqual(["1"]);
  });
});

describe("a new group's memory", () => {
  it("turns what the user wrote into dated entries", () => {
    const memory = memorySeed(
      "",
      "- The client is Nexa\n\n* Deadline: Nov 20\r\n",
      "2026-11-05",
    );
    expect(memory).toContain("The client is Nexa");
    expect(memory).toContain("Deadline: Nov 20");
    expect(memory).toContain("2026-11-05");
    expect(memory).not.toContain("\r");
  });
});
