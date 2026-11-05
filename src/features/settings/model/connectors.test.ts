// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import { buildClaudeSpawnArgs } from "../../../integrations/harness/providers/claude/claudeProtocol";
import {
  askingPermissionMode,
  availableConnectors,
  CONNECTORS,
  connectorAllowedTools,
  connectorAskTools,
  connectorCodexArgs,
  connectorDeniedTools,
  connectorMcpConfig,
  enabledConnectors,
  sessionConnectors,
  setConnectorCheckout,
  setConnectorEnabled,
  userGrantedAdded,
} from "./connectors";

const stored = new Map<string, string>();
vi.stubGlobal("localStorage", {
  clear: () => stored.clear(),
  getItem: (key: string) => stored.get(key) ?? null,
  setItem: (key: string, value: string) => stored.set(key, value),
  removeItem: (key: string) => stored.delete(key),
});

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

describe("more connectors, off until the user turns them on", () => {
  it("adds Cloudflare Docs, Hugging Face, Exa and Playwright, all off", () => {
    const added = byId("cloudflare", "huggingface", "exa", "playwright");
    expect(added.every((connector) => !connector.onByDefault)).toBe(true);
    expect(enabledConnectors().map((connector) => connector.id)).toEqual([
      "context7",
      "xmd",
    ]);
  });

  it("runs Playwright locally at a pinned version", () => {
    const [playwright] = byId("playwright");
    expect(connectorMcpConfig([playwright])).toEqual({
      mcpServers: {
        playwright: {
          type: "stdio",
          command: "npx",
          args: ["-y", "@playwright/mcp@0.0.84"],
        },
      },
    });
    // Clicking and typing are not pre-approved.
    expect(connectorAllowedTools([playwright])).not.toContain(
      "mcp__playwright__browser_click",
    );
  });

  it("hands Codex the same servers as -c overrides with their tools", () => {
    expect(connectorCodexArgs(byId("exa", "playwright"))).toEqual([
      "-c",
      'mcp_servers.exa.url="https://mcp.exa.ai/mcp"',
      "-c",
      'mcp_servers.exa.enabled_tools=["web_search_exa","web_fetch_exa"]',
      "-c",
      'mcp_servers.playwright.command="npx"',
      "-c",
      'mcp_servers.playwright.args=["-y","@playwright/mcp@0.0.84"]',
      "-c",
      expect.stringMatching(
        /^mcp_servers\.playwright\.enabled_tools=\["browser_navigate",/,
      ),
    ]);
    expect(connectorCodexArgs([])).toEqual([]);
  });
});

describe("the iPhone through Poki", () => {
  const [poki] = byId("poki");
  const turnOn = (path?: string) => {
    setConnectorEnabled("poki", true);
    if (path !== undefined) setConnectorCheckout("poki", path);
  };
  afterEach(() => localStorage.clear());

  it("is off, experimental and listed only on a Mac", () => {
    expect(poki.onByDefault).toBe(false);
    expect(poki.experimental).toBe(true);
    const ids = (platform: "mac" | "windows" | "linux") =>
      availableConnectors(platform).map((connector) => connector.id);
    expect(ids("mac")).toContain("poki");
    expect(ids("windows")).not.toContain("poki");
    expect(ids("linux")).not.toContain("poki");
  });

  it("runs only once switched on, on a Mac, with its folder set", () => {
    turnOn();
    expect(enabledConnectors("mac").map((c) => c.id)).not.toContain("poki");
    setConnectorCheckout("poki", "/Users/me/poki");
    expect(enabledConnectors("mac").map((c) => c.id)).toContain("poki");
    expect(enabledConnectors("windows").map((c) => c.id)).not.toContain(
      "poki",
    );
  });

  it("runs node on the checkout's stdio bridge, for Claude and Codex", () => {
    turnOn("/Users/me/poki/");
    expect(connectorMcpConfig([poki])).toEqual({
      mcpServers: {
        poki: {
          type: "stdio",
          command: "node",
          args: ["/Users/me/poki/src/stdio.mjs"],
        },
      },
    });
    expect(connectorCodexArgs([poki]).slice(0, 4)).toEqual([
      "-c",
      'mcp_servers.poki.command="node"',
      "-c",
      'mcp_servers.poki.args=["/Users/me/poki/src/stdio.mjs"]',
    ]);
    // Nothing passes an approval mode: Poki keeps asking on its side too.
    expect(JSON.stringify(connectorMcpConfig([poki]))).not.toMatch(
      /APPROVAL/,
    );
  });

  it("allows only reading the screen; acting asks and the rest is denied", () => {
    expect(connectorAllowedTools([poki])).toEqual(
      [
        "poki_mcp_status",
        "list_devices",
        "select_device",
        "get_foreground_app",
        "list_elements",
        "take_screenshot",
        "list_apps",
        "wait_for",
      ].map((tool) => `mcp__poki__${tool}`),
    );
    const ask = connectorAskTools([poki]);
    expect(ask).toContain("mcp__poki__tap");
    expect(ask).toContain("mcp__poki__fill");
    expect(ask).toContain("mcp__poki__type_text");
    const denied = connectorDeniedTools();
    for (const tool of [
      "batch_commands",
      "get_clipboard",
      "set_clipboard",
      "install_app",
      "uninstall_app",
      "download_app_container",
      "build_and_run",
    ])
      expect(denied).toContain(`mcp__poki__${tool}`);
    const allowed = new Set(connectorAllowedTools([poki]));
    for (const tool of [...ask, ...denied])
      expect(allowed.has(tool)).toBe(false);
    expect(ask.some((tool) => denied.includes(tool))).toBe(false);
  });

  it("reaches only a bot whose list names it", () => {
    turnOn("/Users/me/poki");
    const ids = (only?: string[]) =>
      sessionConnectors(only, "mac").map((connector) => connector.id);
    expect(ids()).not.toContain("poki");
    expect(ids(["xmd"])).toEqual(["xmd"]);
    expect(ids(["poki"])).toEqual(["poki"]);
  });

  it("tells when a list would add it", () => {
    expect(userGrantedAdded(["poki"], undefined).map((c) => c.id)).toEqual([
      "poki",
    ]);
    expect(userGrantedAdded(["poki"], ["xmd"]).map((c) => c.id)).toEqual([
      "poki",
    ]);
    expect(userGrantedAdded(["poki"], ["poki"])).toEqual([]);
    expect(userGrantedAdded([], ["poki"])).toEqual([]);
    expect(userGrantedAdded(undefined, undefined)).toEqual([]);
  });

  it("takes self-approving modes away from a session that has it", () => {
    expect(askingPermissionMode("auto", [poki])).toBe("default");
    expect(askingPermissionMode("bypassPermissions", [poki])).toBe("default");
    expect(askingPermissionMode("plan", [poki])).toBe("plan");
    expect(askingPermissionMode("acceptEdits", [poki])).toBe("acceptEdits");
    expect(askingPermissionMode("auto", byId("xmd", "playwright"))).toBe(
      "auto",
    );
  });
});
