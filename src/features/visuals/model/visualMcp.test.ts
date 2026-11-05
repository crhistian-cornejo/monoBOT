import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => "C:/MonoBOT/monocode.exe"),
}));

import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import {
  VISUAL_INSTRUCTIONS,
  visualMcpServer,
  withVisualServer,
} from "./visualMcp";

describe("visual MCP server", () => {
  it("runs this app's executable beside the session's other servers", async () => {
    const server = await visualMcpServer();
    expect(server).toEqual({
      type: "stdio",
      command: "C:/MonoBOT/monocode.exe",
      args: ["visual-mcp"],
    });
    expect(
      withVisualServer({ mcpServers: { poki: { type: "stdio" } } }, server),
    ).toEqual({
      mcpServers: { poki: { type: "stdio" }, monocode_visual: server },
    });
    expect(withVisualServer(null, null)).toBeNull();
  });

  it("tells Claude to answer with inline visuals, not Artifact pages", () => {
    expect(VISUAL_INSTRUCTIONS).toContain("visual_show");
    expect(VISUAL_INSTRUCTIONS).toContain("rather than as an Artifact");
    // Windows starts claude.cmd, whose arguments cannot hold line breaks.
    expect(VISUAL_INSTRUCTIONS).not.toMatch(/[\r\n]/);
    expect(VISUAL_INSTRUCTIONS).not.toContain("###");
    const args = buildClaudeSpawnArgs({ appendSystemPrompt: VISUAL_INSTRUCTIONS });
    expect(args[args.indexOf("--append-system-prompt") + 1]).toBe(
      VISUAL_INSTRUCTIONS,
    );
    expect(
      buildClaudeSpawnArgs({ isolated: true, appendSystemPrompt: "x" }),
    ).not.toContain("--append-system-prompt");
  });
});
