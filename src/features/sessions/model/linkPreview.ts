import { invoke } from "@tauri-apps/api/core";

export type UserLink = {
  url: string;
  host: string;
  displayUrl: string;
  githubWorkItem?: GithubWorkItemLink;
};

export type GithubWorkItemLink = {
  kind: "pr" | "issue";
  repo: string;
  number: number;
};

export type UserMessageLink = {
  link: UserLink;
  beforeText: string;
  afterText: string;
};

export type LinkPreviewMetadata = {
  title: string | null;
  /** The page's own summary (og:description and the like), when it has one. */
  description?: string | null;
  faviconDataUrl: string | null;
};

const metadataCache = new Map<string, Promise<LinkPreviewMetadata>>();

/** Find the first web URL and preserve the rest of the user's message. */
export function parseUserMessageLink(text: string): UserMessageLink | null {
  const match = /https?:\/\/[^\s<>"']+/i.exec(text);
  if (!match) return null;

  const value = trimUrlPunctuation(match[0]);
  const link = parseHttpUrl(value);
  if (!link) return null;

  return {
    link,
    beforeText: text.slice(0, match.index),
    afterText: text.slice(match.index + value.length),
  };
}

/** Kept as a small utility for callers that need the stricter exact-URL rule. */
export function parseStandaloneHttpUrl(text: string): UserLink | null {
  const value = text.trim();
  if (!value || /\s/.test(value)) return null;
  return parseHttpUrl(value);
}

function parseHttpUrl(value: string): UserLink | null {
  let parsed: URL;
  try {
    parsed = new URL(value);
  } catch {
    return null;
  }

  if (
    (parsed.protocol !== "https:" && parsed.protocol !== "http:") ||
    parsed.username ||
    parsed.password ||
    !parsed.hostname
  ) {
    return null;
  }

  const host = parsed.hostname.replace(/\.$/, "").toLowerCase();
  const displayHost = host.startsWith("www.") ? host.slice(4) : host;
  const suffix = `${parsed.pathname === "/" ? "" : parsed.pathname}${parsed.search}${parsed.hash}`;
  const workItem = githubWorkItem(parsed, displayHost);

  return {
    url: parsed.href,
    host: displayHost,
    displayUrl: `${displayHost}${suffix}`,
    ...(workItem ? { githubWorkItem: workItem } : {}),
  };
}

function githubWorkItem(
  url: URL,
  displayHost: string,
): GithubWorkItemLink | null {
  if (displayHost !== "github.com") return null;
  const parts = url.pathname.split("/").filter(Boolean);
  if (parts.length < 4) return null;
  const resource = parts[2]?.toLowerCase();
  if (resource !== "pull" && resource !== "issues") return null;
  const numberText = parts[3] ?? "";
  if (!/^[1-9]\d*$/.test(numberText)) return null;
  const number = Number(numberText);
  if (!Number.isSafeInteger(number)) return null;

  const owner = decodePathPart(parts[0] ?? "");
  const name = decodePathPart(parts[1] ?? "");
  if (!owner || !name || /[\s/]/.test(owner) || /[\s/]/.test(name)) return null;

  return {
    kind: resource === "pull" ? "pr" : "issue",
    repo: `${owner}/${name}`,
    number,
  };
}

function decodePathPart(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return "";
  }
}

function trimUrlPunctuation(value: string): string {
  let end = value.length;
  while (end > 0 && /[.,!;:]/.test(value[end - 1])) end -= 1;

  for (const [opening, closing] of [
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
  ]) {
    while (
      value.slice(0, end).endsWith(closing) &&
      count(value.slice(0, end), closing) > count(value.slice(0, end), opening)
    ) {
      end -= 1;
    }
  }
  return value.slice(0, end);
}

function count(value: string, character: string): number {
  return value.split(character).length - 1;
}

export function fetchLinkPreviewMetadata(
  url: string,
): Promise<LinkPreviewMetadata> {
  const cached = metadataCache.get(url);
  if (cached) return cached;

  const pending = invoke<LinkPreviewMetadata>("fetch_link_preview", { url });
  metadataCache.set(url, pending);
  pending.catch(() => metadataCache.delete(url));
  return pending;
}

/** A site's icon by host; null once the site has none or could not be read. */
const siteIconCache = new Map<string, string | null>();
const siteIconPending = new Map<string, Promise<string | null>>();

/** The host a source's icon belongs to: `www.` dropped, a full URL accepted. */
export function siteIconHost(hostOrUrl: string): string | null {
  const value = hostOrUrl.trim().toLowerCase();
  if (!value) return null;
  let host = value;
  if (value.includes("://")) {
    try {
      const parsed = new URL(value);
      if (parsed.protocol !== "https:" && parsed.protocol !== "http:")
        return null;
      host = parsed.hostname;
    } catch {
      return null;
    }
  }
  host = host.replace(/\.$/, "").replace(/^www\./, "");
  // A bare name or anything with a path is not a site to ask for an icon.
  return /^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(host) ? host : null;
}

/** The icon already known for a host, so a remounted chip paints it at once. */
export function cachedSiteIcon(hostOrUrl: string): string | null | undefined {
  const host = siteIconHost(hostOrUrl);
  return host ? siteIconCache.get(host) : null;
}

/**
 * A site's icon, read once per host from its home page: a cited PDF or paper
 * has no icon of its own, but the site it sits on does. A failure is kept too,
 * so a long transcript does not ask the same site again.
 */
export function fetchSiteIcon(hostOrUrl: string): Promise<string | null> {
  const host = siteIconHost(hostOrUrl);
  if (!host) return Promise.resolve(null);
  if (siteIconCache.has(host)) return Promise.resolve(siteIconCache.get(host)!);
  const pending = siteIconPending.get(host);
  if (pending) return pending;
  const next = fetchLinkPreviewMetadata(`https://${host}/`)
    .then((meta) => meta.faviconDataUrl)
    .catch(() => null)
    .then((icon) => {
      siteIconCache.set(host, icon);
      siteIconPending.delete(host);
      return icon;
    });
  siteIconPending.set(host, next);
  return next;
}
