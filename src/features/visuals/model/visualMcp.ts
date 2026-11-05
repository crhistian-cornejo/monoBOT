// The `monocode_visual` MCP server an agent publishes visuals through. It is
// this app's own executable (`monocode visual-mcp`, src-tauri/src/visual_cli.rs),
// which reaches the chat through the turn's MONOCODE_APP_* connection.
import { invoke } from "@tauri-apps/api/core";

// When to reach for a visual; the native app gives its agents the same rule.
import usage from "../../../../crates/monocode-core/src/visual/usage.md?raw";

export const VISUAL_SERVER = "monocode_visual";
/** Claude may check and show visuals without asking: they change nothing. */
export const VISUAL_ALLOWED_TOOLS = `mcp__${VISUAL_SERVER}`;

/**
 * Added to the agent's system prompt. Claude Code also offers its own
 * Artifact pages; a chart for this chat belongs in the chat instead.
 */
export const VISUAL_INSTRUCTIONS = `${usage
  // One line: on Windows Claude starts through a .cmd, and batch-file
  // arguments cannot carry line breaks (Rust refuses to spawn them).
  .replace(/^#+[^\n]*\n/, "")
  .replace(/\s+/g, " ")
  .trim()} In MonoBOT, show charts, diagrams and visualizers this way rather than as an Artifact, a claude.ai page or an HTML file, unless the user asks for one of those. The ${VISUAL_SERVER} tools may be deferred: load them with ToolSearch before you call them. Put the embed line visual_show returns on its own line in your reply.`;

export type VisualMcpServer = {
  type: "stdio";
  command: string;
  args: string[];
};

let executable: Promise<string | null> | undefined;

/** The server definition, or null outside the desktop app. */
export async function visualMcpServer(): Promise<VisualMcpServer | null> {
  executable ??= invoke<string>("app_cli_path").catch(() => null);
  const command = await executable;
  return command ? { type: "stdio", command, args: ["visual-mcp"] } : null;
}

/** `config` with the visual server beside its other servers. */
export function withVisualServer<T extends { mcpServers: Record<string, unknown> }>(
  config: T | null | undefined,
  server: VisualMcpServer | null,
): { mcpServers: Record<string, unknown> } | null | undefined {
  if (!server) return config;
  return { mcpServers: { ...(config?.mcpServers ?? {}), [VISUAL_SERVER]: server } };
}
