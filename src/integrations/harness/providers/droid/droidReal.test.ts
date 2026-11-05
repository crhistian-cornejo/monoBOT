// Real-endpoint integration test: drives the actual adapter against the
// installed `droid` over stdio. Skipped unless DROID_REAL=1, so CI and
// machines without Droid (or without a Factory login) are unaffected.
import { mkdtempSync, readFileSync, existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import type { HarnessEvent, SendTurnInput } from "../../core/types";

const REAL = process.env.DROID_REAL === "1";
const BIN = process.env.DROID_BIN ?? "droid";

const live = vi.hoisted(() => ({
  children: new Map<string, import("node:child_process").ChildProcess>(),
  listeners: new Map<string, (line: string) => void>(),
  exits: new Map<string, (code: number | null) => void>(),
}));

vi.mock("../../core/child", async () => {
  const { spawn } = await import("node:child_process");
  const readline = await import("node:readline");
  return {
    resolveDroidBinary: async () => ({
      path: BIN,
      args: ["exec", "-o", "acp"],
    }),
    spawnChild: async (id: string, command: string, args: string[], cwd: string) => {
      // npm's launcher on Windows is a .cmd, which needs a shell to start.
      const child = spawn(command, args, { cwd, shell: process.platform === "win32" });
      live.children.set(id, child);
      readline
        .createInterface({ input: child.stdout! })
        .on("line", (line) => live.listeners.get(id)?.(line));
      child.on("exit", (code) => live.exits.get(id)?.(code));
    },
    killChild: async (id: string) => {
      live.children.get(id)?.kill();
      live.children.delete(id);
    },
    writeChild: async (id: string, line: string) => {
      const child = live.children.get(id);
      if (!child) throw new Error(`no child for ${id}`);
      await new Promise<void>((resolve, reject) =>
        child.stdin!.write(line + "\n", (error) => (error ? reject(error) : resolve())),
      );
    },
    watchChild: (id: string, onLine: (line: string) => void, onExit?: (code: number | null) => void) => {
      live.listeners.set(id, onLine);
      if (onExit) live.exits.set(id, onExit);
    },
    unwatchChild: (id: string) => {
      live.listeners.delete(id);
      live.exits.delete(id);
    },
  };
});
vi.mock("../../../../platform/tauri/fs", () => ({ homeDir: async () => homedir() }));

const droid = await import("./droid");
const catalog = await import("./droidCatalog");

function turn(
  sessionId: string,
  cwd: string,
  text: string,
  runtimeMode: SendTurnInput["runtimeMode"],
  events: HarnessEvent[],
): SendTurnInput {
  return {
    sessionId,
    cwd,
    text,
    attachments: [],
    model: "droid:claude-haiku-5-5",
    modelSettings: {},
    runtimeMode,
    onEvent: (event) => events.push(event),
  } as unknown as SendTurnInput;
}

const replyOf = (events: HarnessEvent[]) =>
  events
    .flatMap((event) =>
      event.type === "message.delta" && "text" in event ? [String(event.text)] : [],
    )
    .join("");

describe.runIf(REAL)("droid over ACP (real)", () => {
  it("lists its models", async () => {
    const models = await catalog.discoverDroidModels(tmpdir());
    expect(models.length).toBeGreaterThan(5);
    expect(models.some((model) => model.id === "droid:auto")).toBe(true);
  }, 90_000);

  it("answers, keeps the conversation, and asks before editing when supervised", async () => {
    const cwd = mkdtempSync(join(tmpdir(), "monobot-droid-"));
    const events: HarnessEvent[] = [];
    await droid.sendDroidTurn(
      turn("real-1", cwd, "Reply with exactly: hola desde droid", "supervised", events),
    );
    expect(replyOf(events).toLowerCase()).toContain("hola desde droid");
    expect(events.some((event) => event.type === "session.providerBound")).toBe(true);

    // Supervised: an edit is put to the user; denying it leaves no file.
    const second: HarnessEvent[] = [];
    const input = turn(
      "real-1",
      cwd,
      "Create a file named probe.txt containing hola. Then say done.",
      "supervised",
      second,
    );
    input.onEvent = (event) => {
      second.push(event);
      if (event.type === "approval.requested")
        droid.respondDroidApproval("real-1", event.requestId, "deny");
    };
    await droid.sendDroidTurn(input);
    expect(second.some((event) => event.type === "approval.requested")).toBe(true);
    expect(existsSync(join(cwd, "probe.txt"))).toBe(false);

    // Full access: Droid edits on its own, in the same conversation.
    const third: HarnessEvent[] = [];
    await droid.sendDroidTurn(
      turn("real-1", cwd, "Create a file named probe.txt containing hola. Then say done.", "full-access", third),
    );
    expect(readFileSync(join(cwd, "probe.txt"), "utf8")).toContain("hola");
    await droid.stopDroidSession("real-1");
  }, 240_000);
});
