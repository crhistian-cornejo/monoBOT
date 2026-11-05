import { describe, expect, it } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import {
  CONNECTORS,
  connectorAllowedTools,
  connectorMcpConfig,
  enabledConnectors,
} from "./connectors";

const byId = (...ids: string[]) =>
  CONNECTORS.filter((connector) => ids.includes(connector.id));

describe("connectors", () => {
  it("enables the on-by-default connectors when nothing is saved", () => {
    expect(enabledConnectors().map((connector) => connector.id)).toEqual([
      "context7",
      "xmd",
    ]);
  });

  it("builds an HTTP MCP config and allows only the reviewed tools", () => {
    const connectors = byId("context7", "deepwiki");
    expect(connectorMcpConfig(connectors)).toEqual({
      mcpServers: {
        context7: { type: "http", url: "https://mcp.context7.com/mcp" },
        deepwiki: { type: "http", url: "https://mcp.deepwiki.com/mcp" },
      },
    });
    expect(connectorAllowedTools(connectors)).toEqual([
      "mcp__context7__resolve-library-id",
      "mcp__context7__query-docs",
      "mcp__deepwiki__read_wiki_structure",
      "mcp__deepwiki__read_wiki_contents",
      "mcp__deepwiki__ask_question",
    ]);
  });

  it("keeps the community-hosted OpenAlex connector off until the user opts in", () => {
    const [openalex] = byId("openalex");
    expect(openalex.onByDefault).toBe(false);
    expect(openalex.url).toMatch(/^https:\/\//);
    expect(connectorAllowedTools([openalex])).toEqual(
      openalex.tools.map((tool) => `mcp__openalex__${tool}`),
    );
  });

  it("adds nothing when every connector is off", () => {
    expect(connectorMcpConfig([])).toBeNull();
    expect(connectorAllowedTools([])).toEqual([]);
  });

  it("passes connectors to interactive turns beside the user's servers", () => {
    const connectors = byId("xmd");
    const args = buildClaudeSpawnArgs({
      mcpConfig: connectorMcpConfig(connectors),
      allowedTools: connectorAllowedTools(connectors),
    });
    const config = args[args.indexOf("--mcp-config") + 1];
    expect(JSON.parse(config)).toEqual(connectorMcpConfig(connectors));
    expect(args).not.toContain("--strict-mcp-config");
    expect(args[args.indexOf("--allowedTools") + 1]).toContain(
      "mcp__xmd__x_md_get_post",
    );
  });

  it("keeps isolated helper calls free of every MCP server", () => {
    const connectors = byId("xmd");
    const args = buildClaudeSpawnArgs({
      isolated: true,
      mcpConfig: connectorMcpConfig(connectors),
      allowedTools: connectorAllowedTools(connectors),
    });
    expect(args[args.indexOf("--mcp-config") + 1]).toBe(
      JSON.stringify({ mcpServers: {} }),
    );
    expect(args).not.toContain("--allowedTools");
  });
});
