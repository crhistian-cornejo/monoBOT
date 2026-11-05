// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";
import { newSession, type Block } from "../../sessions/model/session";
import { findMono, saveGroup } from "../../monos/model/mono";
import { answerAccessRequest } from "../../groups/model/group";
import { handleAgentApp, type AgentAppHost } from "./agentApp";
import { replyAfter } from "./agentMessages";

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

const ROSTER = [
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
  {
    id: "nexa",
    sessionId: "nexa-chat",
    name: "Nexa",
    mascot: "mushroom",
    color: "purple",
    projects: [],
    kind: "group",
    group: {
      members: ["research", "docs"],
      goal: "Answer the authority's observations",
      folders: [{ path: "D:\\Projects\\1064", edit: false }],
    },
  },
];

beforeEach(() => {
  stored.clear();
  stored.set("monocode:mono-roster", JSON.stringify(ROSTER));
  stored.set("monocode:principal-tools-reset", "1");
});

/** A host whose agents answer at once, in their own chats. */
function host(): AgentAppHost & { chats: Map<string, Block[]> } {
  const chats = new Map<string, Block[]>();
  const sessionOf: Record<string, string> = {
    research: "research-chat",
    docs: "docs-chat",
  };
  return {
    chats,
    start: vi.fn(async () => {}),
    sessions: vi.fn(async () => []),
    session: vi.fn(async (id: string) => {
      const blocks = chats.get(id);
      return blocks
        ? { ...newSession("claude", "/home"), id, blocks, busy: false }
        : null;
    }),
    send: vi.fn(async (id: string, prompt: string, requestId: string) => {
      chats.set(id, [
        { id: "q", role: "user", text: prompt, appRequestId: requestId },
        { id: "a", role: "assistant", text: `answer from ${id}` },
      ]);
      return { alreadySubmitted: false };
    }),
    draft: vi.fn(async () => ({ alreadySaved: false, draft: true })),
    stop: vi.fn(async () => {}),
    remove: vi.fn(async () => {}),
    worktrees: vi.fn(async () => ({ worktrees: [], defaultRoot: "" })),
    createWorktree: vi.fn(),
    notes: vi.fn(async () => []),
    note: vi.fn(async () => null),
    saveNote: vi.fn(),
    isMono: () => true,
    monoOf: (sessionId: string) => {
      const mono = ROSTER.find((entry) => entry.sessionId === sessionId);
      return mono ? { id: mono.id, projects: [] } : undefined;
    },
    monoSession: async (monoId: string) => sessionOf[monoId],
  } as unknown as AgentAppHost & { chats: Map<string, Block[]> };
}

describe("a group as a project", () => {
  it("puts the Principal in every group, leading", () => {
    expect(findMono("nexa")?.group?.members).toEqual([
      "ivy",
      "research",
      "docs",
    ]);
  });

  it("answers access from the folders and asks the user for the rest", async () => {
    const app = host();
    const inside = (await handleAgentApp(
      { id: "nexa-chat" },
      "a1",
      "group.access",
      { target: "D:\\Projects\\1064\\planos" },
      app,
    )) as { access: string };
    expect(inside.access).toBe("read");

    const asked = (await handleAgentApp(
      { id: "research-chat" },
      "a2",
      "group.access",
      { target: "E:\\Survey", reason: "The survey drawings" },
      app,
    )) as { access: string; requestId: string };
    expect(asked.access).toBe("asked");
    const request = findMono("nexa")?.group?.requests?.[0];
    expect(request).toMatchObject({
      target: "E:\\Survey",
      by: "Research",
      reason: "The survey drawings",
    });

    // Only the user's tap answers it.
    saveGroup("nexa", (group) =>
      answerAccessRequest(group, asked.requestId, "edit"),
    );
    const again = (await handleAgentApp(
      { id: "docs-chat" },
      "a3",
      "group.access",
      { target: "E:\\Survey\\cap3.docx" },
      app,
    )) as { access: string };
    expect(again.access).toBe("edit");
  });

  it("lets the leader wait for a member's answer", async () => {
    const app = host();
    const result = (await handleAgentApp(
      { id: "nexa-chat" },
      "ask1",
      "group.ask",
      { member: "Research", message: "Find the drawings", wait: true },
      app,
    )) as { member: string; reply: string };
    expect(result).toMatchObject({
      member: "Research",
      reply: "answer from research-chat",
    });
    // A waited answer does not come back again as a completion.
    expect(app.send).toHaveBeenCalledWith(
      "research-chat",
      expect.stringContaining("D:\\Projects\\1064 (read only)"),
      "app-nexa-chat-ask1",
    );
  });

  it("does not let the leader ask the Principal it speaks for", async () => {
    await expect(
      handleAgentApp(
        { id: "nexa-chat" },
        "ask2",
        "group.ask",
        { member: "IVY", message: "Hi" },
        host(),
      ),
    ).rejects.toThrow("No member by that name");
  });

  it("lets members of one group ask each other while they answer", async () => {
    const app = host();
    // Research is answering Documents...
    await handleAgentApp(
      { id: "docs-chat" },
      "m1",
      "agents.message",
      { to: "Research", message: "Which drawing is current?" },
      app,
    );
    // ...and may still ask Documents back, waiting for the reply.
    const result = (await handleAgentApp(
      { id: "research-chat" },
      "m2",
      "agents.message",
      { to: "Documents", message: "Send me the Word file", wait: true },
      app,
    )) as { reply: string };
    expect(result.reply).toBe("answer from docs-chat");
  });
});

describe("reading an agent's answer", () => {
  const blocks: Block[] = [
    { id: "0", role: "assistant", text: "earlier" },
    { id: "1", role: "user", text: "q", appRequestId: "r" },
    { id: "2", role: "assistant", text: "Looking" },
    { id: "3", role: "assistant", text: "", tool: { title: "Read" } },
    { id: "4", role: "assistant", text: "Found it" },
    { id: "5", role: "user", text: "next" },
    { id: "6", role: "assistant", text: "other" },
  ];

  it("takes its words up to the next message, once its turn is over", () => {
    expect(replyAfter({ blocks, busy: false }, "r")).toEqual({
      done: true,
      text: "Looking\n\nFound it",
    });
    expect(replyAfter({ blocks, busy: true }, "r")?.done).toBe(false);
    expect(replyAfter({ blocks: blocks.slice(0, 2), busy: false }, "r")).toEqual(
      { done: false, text: "" },
    );
    expect(replyAfter({ blocks, busy: false }, "missing")).toBeUndefined();
  });
});
