import type { Block } from "../../sessions/model/session";
import { citationHost, collectCitations } from "../../sessions/model/citations";

/**
 * What a turn looked at, read from its tool calls: the web searches it ran,
 * the pages it opened and which of them its answer ended up citing. The chat
 * shows the answer; this is the legwork behind it.
 */

export type ResearchSearch = {
  id: string;
  query: string;
  results: number;
};

export type ResearchSource = {
  url: string;
  host: string;
  title?: string;
  /** Its number in the answer's citations, when the answer cites it. */
  cited?: number;
  /** The agent opened the page, rather than only seeing it in results. */
  read: boolean;
};

export type TurnResearch = {
  searches: ResearchSearch[];
  sources: ResearchSource[];
  /** Search results it neither opened nor cited. */
  skipped: number;
  /** The block of the answer, which the chat already shows. */
  answerId?: string;
};

const HTTP = /^https?:\/\//i;

function searchLinks(
  detail: string | undefined,
): { title?: string; url: string }[] {
  if (!detail) return [];
  const at = detail.indexOf("Links:");
  if (at < 0) return [];
  const start = detail.indexOf("[", at);
  if (start < 0) return [];
  const end = detail.indexOf("\n", start);
  const raw = detail.slice(start, end < 0 ? undefined : end).trim();
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.flatMap((entry) => {
      const url = (entry as { url?: unknown })?.url;
      const title = (entry as { title?: unknown })?.title;
      return typeof url === "string" && HTTP.test(url)
        ? [
            {
              url,
              ...(typeof title === "string" && title.trim()
                ? { title: title.trim() }
                : {}),
            },
          ]
        : [];
    });
  } catch {
    return [];
  }
}

function fetchedUrl(block: Block): string | undefined {
  const preview = block.tool?.preview;
  const url = preview?.url ?? preview?.path;
  return url && HTTP.test(url) ? url : undefined;
}

const sameUrl = (url: string) =>
  url
    .replace(/[#?].*$/, "")
    .replace(/\/+$/, "")
    .toLowerCase();

/** The last thing the agent said: the answer the chat shows. */
export function answerBlock(turn: Block[]): Block | undefined {
  for (let index = turn.length - 1; index >= 0; index -= 1) {
    const block = turn[index];
    if (block.role === "assistant" && !block.internal && block.text.trim())
      return block;
  }
  return undefined;
}

export function turnResearch(turn: Block[]): TurnResearch {
  const searches: ResearchSearch[] = [];
  const sources = new Map<string, ResearchSource>();
  const titles = new Map<string, string>();
  let seen = 0;
  const add = (url: string, change: Partial<ResearchSource>) => {
    const key = sameUrl(url);
    const known = sources.get(key);
    sources.set(key, {
      url: known?.url ?? url,
      host: citationHost(url),
      read: false,
      ...known,
      ...change,
      ...(known?.read || change.read ? { read: true } : {}),
    });
  };
  for (const block of turn) {
    if (block.role !== "tool" || !block.tool) continue;
    const preview = block.tool.preview;
    if (preview?.kind === "search" && preview.query) {
      const links = searchLinks(block.tool.detail);
      for (const link of links) {
        if (link.title) titles.set(sameUrl(link.url), link.title);
      }
      seen += links.length;
      searches.push({
        id: block.id,
        query: preview.query,
        results: links.length,
      });
      continue;
    }
    const url = fetchedUrl(block);
    if (url) add(url, { read: true });
  }
  const answer = answerBlock(turn);
  for (const cited of answer ? collectCitations(answer.text) : []) {
    add(cited.url, {
      cited: cited.number,
      ...(cited.title && cited.title !== cited.host
        ? { title: cited.title }
        : {}),
    });
  }
  const list = [...sources.values()].map((source) => ({
    ...source,
    title: source.title ?? titles.get(sameUrl(source.url)),
  }));
  list.sort(
    (a, b) =>
      (a.cited ?? Number.MAX_SAFE_INTEGER) -
        (b.cited ?? Number.MAX_SAFE_INTEGER) || Number(b.read) - Number(a.read),
  );
  const used = new Set(list.map((source) => sameUrl(source.url)));
  const resultUrls = new Set<string>();
  for (const block of turn) {
    if (block.tool?.preview?.kind !== "search") continue;
    for (const link of searchLinks(block.tool.detail))
      resultUrls.add(sameUrl(link.url));
  }
  const skipped = [...resultUrls].filter((url) => !used.has(url)).length;
  return {
    searches,
    sources: list,
    skipped: seen ? skipped : 0,
    ...(answer ? { answerId: answer.id } : {}),
  };
}
