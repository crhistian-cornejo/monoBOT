import { readFlag, writeFlag } from "./storageFlags";

/** Built-in remote MCP servers (Streamable HTTP) that need nothing installed
 * and no account. MonoBOT passes the enabled ones to each Claude turn with
 * `--mcp-config`, so the user's own `~/.claude.json` is never touched. They
 * are network tools: what they return is data, never instructions. */
export type ConnectorId =
  | "context7"
  | "mslearn"
  | "deepwiki"
  | "xmd"
  | "openalex";

export type Connector = {
  id: ConnectorId;
  name: string;
  description: string;
  url: string;
  site: string;
  onByDefault: boolean;
  /** Reviewed read-only tools. Never allow a whole server: it may also write. */
  tools: readonly string[];
};

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
        { type: "http", url: connector.url },
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
