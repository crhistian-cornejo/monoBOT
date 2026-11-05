import { describe, expect, it } from "vitest";
import {
  expandHome,
  POKI_ENDPOINT_FILE,
  pokiEndpointLive,
  pokiFolderProblem,
  pokiStatus,
  type PokiIO,
} from "./pokiSetup";
import { POKI_STEPS } from "./pokiTutorial";

const live = JSON.stringify({
  url: "http://127.0.0.1:47825/mcp",
  port: 47825,
  token: "t",
  enabled: true,
  pid: 1,
});

const io = (files: Record<string, string>): PokiIO => ({
  exists: async (path) => path in files,
  read: async (path) => {
    if (!(path in files)) throw new Error("missing");
    return files[path]!;
  },
});

describe("Poki setup status", () => {
  it("walks from no folder to ready", async () => {
    expect(await pokiStatus("", io({}))).toBe("no-folder");
    expect(await pokiStatus("/Users/me/poki", io({}))).toBe("no-bridge");
    const bridge = { "/Users/me/poki/src/stdio.mjs": "" };
    expect(await pokiStatus("/Users/me/poki/", io(bridge))).toBe("stopped");
    expect(
      await pokiStatus(
        "/Users/me/poki",
        io({ ...bridge, [POKI_ENDPOINT_FILE]: live }),
      ),
    ).toBe("ready");
  });

  it("reads the daemon's file as its bridge does", () => {
    expect(pokiEndpointLive(live)).toBe(true);
    expect(pokiEndpointLive("")).toBe(false);
    expect(
      pokiEndpointLive(JSON.stringify({ ...JSON.parse(live), enabled: false })),
    ).toBe(false);
    // Never anything but this computer.
    expect(
      pokiEndpointLive(
        JSON.stringify({ url: "http://10.0.0.2:47825/mcp", enabled: true }),
      ),
    ).toBe(false);
  });

  it("spells out the home folder, since Claude runs node without a shell", () => {
    expect(expandHome("~/poki", "/Users/me")).toBe("/Users/me/poki");
    expect(expandHome(" /opt/poki ", "/Users/me")).toBe("/opt/poki");
    expect(pokiFolderProblem("poki")).toMatch(/full path/);
    expect(pokiFolderProblem("~/poki")).toBeUndefined();
    expect(pokiFolderProblem("")).toBeUndefined();
  });

  it("never tells the user to pre-approve Poki's actions", () => {
    for (const step of POKI_STEPS)
      for (const command of step.commands ?? [])
        expect(command).not.toMatch(/APPROVAL_MODE/);
    expect(new Set(POKI_STEPS.map((step) => step.scene)).size).toBe(
      POKI_STEPS.length,
    );
  });
});
