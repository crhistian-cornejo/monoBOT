import { readFlag, writeFlag } from "./storageFlags";

/** Built-in MCP servers that need no account: remote ones (Streamable HTTP)
 * and a few that run locally through `npx`. MonoBOT passes the enabled ones
 * to each Claude turn with `--mcp-config` and to Codex as `-c` overrides, so
 * neither CLI's own configuration is touched. What they return is data,
 * never instructions. */
export type ConnectorId =
  | "context7"
  | "mslearn"
  | "deepwiki"
  | "xmd"
  | "openalex"
  | "cloudflare"
  | "huggingface"
  | "exa"
  | "playwright";

export type Connector = {
  id: ConnectorId;
  name: string;
  description: string;
  site: string;
  onByDefault: boolean;
  /** Reviewed read-only tools. Never allow a whole server: it may also write. */
  tools: readonly string[];
} & (
  | { url: string; command?: never }
  /** Runs on this machine; the version is pinned so a turn never upgrades it. */
  | { url?: never; command: string; args: readonly string[] }
);

export const CONNECTORS: readonly Connector[] = [
  {
    id: "context7",
    name: "Context7",
    description: "Up-to-date documentation for libraries and APIs.",
    url: "https://mcp.context7.com/mcp",
    site: "https://context7.com",
    onByDefault: true,
    tools: ["resolve-library-id", "query-docs"],
  },
  {
    id: "mslearn",
    name: "Microsoft Learn",
    description:
      "Official Microsoft docs: Windows, Office, Azure, .NET and PowerShell.",
    url: "https://learn.microsoft.com/api/mcp",
    site: "https://learn.microsoft.com/training/support/mcp",
    onByDefault: false,
    tools: [
      "microsoft_docs_search",
      "microsoft_docs_fetch",
      "microsoft_code_sample_search",
    ],
  },
  {
    id: "deepwiki",
    name: "DeepWiki",
    description: "Explains any public GitHub repository and answers questions.",
    url: "https://mcp.deepwiki.com/mcp",
    site: "https://deepwiki.com",
    onByDefault: false,
    tools: ["read_wiki_structure", "read_wiki_contents", "ask_question"],
  },
  {
    id: "xmd",
    name: "x.md",
    description: "Public X/Twitter posts, threads, profiles and searches.",
    url: "https://mdfromx.com/mcp",
    site: "https://mdfromx.com",
    onByDefault: true,
    tools: [
      "x_md_get_post",
      "x_md_get_profile",
      "x_md_search_posts",
      "x_md_get_followers",
      "x_md_get_following",
    ],
  },
  {
    id: "openalex",
    name: "OpenAlex",
    description:
      "Scholarly papers, authors, citations and trends. Community-hosted: your queries reach that server.",
    url: "https://openalex.caseyjhand.com/mcp",
    site: "https://github.com/cyanheads/openalex-mcp-server",
    onByDefault: false,
    tools: [
      "openalex_resolve_name",
      "openalex_search_entities",
      "openalex_analyze_trends",
      "openalex_get_citation_graph",
      "openalex_describe_fields",
    ],
  },
  {
    id: "cloudflare",
    name: "Cloudflare Docs",
    description:
      "Search Cloudflare's documentation: Workers, Pages, R2, D1 and more.",
    url: "https://docs.mcp.cloudflare.com/mcp",
    site: "https://github.com/cloudflare/mcp-server-cloudflare",
    onByDefault: false,
    tools: [
      "search_cloudflare_documentation",
      "migrate_pages_to_workers_guide",
    ],
  },
  {
    id: "huggingface",
    name: "Hugging Face",
    description: "Search models, datasets and Spaces on the Hugging Face Hub.",
    url: "https://huggingface.co/mcp",
    site: "https://huggingface.co/settings/mcp",
    onByDefault: false,
    tools: ["hub_repo_search", "hub_repo_details", "hf_fs"],
  },
  {
    id: "exa",
    name: "Exa",
    description:
      "Web search and page reading built for agents. Your queries reach Exa; it limits use without a key.",
    url: "https://mcp.exa.ai/mcp",
    site: "https://exa.ai",
    onByDefault: false,
    tools: ["web_search_exa", "web_fetch_exa"],
  },
  {
    id: "playwright",
    name: "Playwright",
    description:
      "A real browser on this computer to open, read and screenshot pages. Runs with npx; clicking and typing still ask.",
    command: "npx",
    args: ["-y", "@playwright/mcp@0.0.84"],
    site: "https://github.com/microsoft/playwright-mcp",
    onByDefault: false,
    tools: [
      "browser_navigate",
      "browser_navigate_back",
      "browser_snapshot",
      "browser_take_screenshot",
      "browser_console_messages",
      "browser_network_requests",
      "browser_resize",
      "browser_tabs",
      "browser_wait_for",
      "browser_close",
    ],
  },
];

const enabledKey = (id: ConnectorId) => `monocode.connector.${id}.enabled`;

export function isConnectorEnabled(connector: Connector): boolean {
  return readFlag(enabledKey(connector.id)) ?? connector.onByDefault;
}

export function setConnectorEnabled(id: ConnectorId, enabled: boolean) {
  writeFlag(enabledKey(id), enabled);
}

export function enabledConnectors(): Connector[] {
  return CONNECTORS.filter(isConnectorEnabled);
}

/** The `--mcp-config` value for these connectors, or null when there are none. */
export function connectorMcpConfig(connectors: readonly Connector[]) {
  if (connectors.length === 0) return null;
  return {
    mcpServers: Object.fromEntries(
      connectors.map((connector) => [
        connector.id,
        connector.command
          ? {
              type: "stdio",
              command: connector.command,
              args: [...connector.args],
            }
          : { type: "http", url: connector.url },
      ]),
    ),
  };
}

/** Claude tool names (`mcp__<server>__<tool>`) the turn may call unprompted. */
export function connectorAllowedTools(connectors: readonly Connector[]) {
  return connectors.flatMap((connector) =>
    connector.tools.map((tool) => `mcp__${connector.id}__${tool}`),
  );
}

/**
 * The same servers as Codex `-c` overrides (`mcp_servers.<id>.url=...`),
 * each limited to its reviewed tools. Values are TOML, so strings are quoted.
 */
export function connectorCodexArgs(connectors: readonly Connector[]): string[] {
  const toml = (value: string) => JSON.stringify(value);
  const list = (values: readonly string[]) => `[${values.map(toml).join(",")}]`;
  return connectors.flatMap((connector) => {
    const key = `mcp_servers.${connector.id}`;
    const where = connector.command
      ? [
          "-c",
          `${key}.command=${toml(connector.command)}`,
          "-c",
          `${key}.args=${list(connector.args)}`,
        ]
      : ["-c", `${key}.url=${toml(connector.url ?? "")}`];
    return [...where, "-c", `${key}.enabled_tools=${list(connector.tools)}`];
  });
}
