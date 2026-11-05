/**
 * An agent cites a source as a numbered markdown link whose title carries the
 * source's name and, after a " | ", the passage that backs the claim:
 *
 *   The curve matters [1](https://example.com/a "Great animations | Most UI motion…").
 *
 * Anywhere else (a docx, a PDF, another viewer) it is still a plain link, so
 * nothing about the format needs the app to be read.
 */

const CITATION_LABEL = /^\[?(\d{1,3})\]?$/;
// A lone "|" between spaces; a pipe inside a word (a|b) is part of the title.
const SNIPPET_SEPARATOR = /(?:^|\s)\|(?:\s|$)/;

export type CitationText = { title: string; snippet?: string };

/** The number in a link label such as `1` or `[12]`, or null for any other link. */
export function citationNumber(label: string): number | null {
  const match = CITATION_LABEL.exec(label.trim());
  return match ? Number(match[1]) : null;
}

/** Only web sources are citations; a file link stays a file link. */
export function isCitationHref(href: string | undefined): href is string {
  return !!href && /^https?:\/\//i.test(href);
}

export function citationHost(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

export type CitationSource = CitationText & {
  url: string;
  host: string;
  /** Order of first citation, from 1. The agent's own numbers are not trusted. */
  number: number;
};

const CITATION_LINK =
  /\[\[?(\d{1,3})\]?\]\((https?:\/\/[^\s)]+)(?:\s+"((?:[^"\\]|\\.)*)")?\)/g;

/**
 * The distinct sources a text cites, numbered in the order it first cites
 * them. Code is skipped, so an example of the format is not a citation.
 */
export function collectCitations(markdown: string): CitationSource[] {
  const text = markdown
    .replace(/```[\s\S]*?(?:```|$)/g, "")
    .replace(/`[^`\n]*`/g, "");
  const sources = new Map<string, CitationSource>();
  for (const match of text.matchAll(CITATION_LINK)) {
    const url = match[2];
    const parsed = parseCitationTitle(match[3]?.replace(/\\"/g, '"'), url);
    const known = sources.get(url);
    if (known) {
      // A later mention may carry the passage the first one lacked.
      if (!known.snippet && parsed.snippet) known.snippet = parsed.snippet;
      continue;
    }
    sources.set(url, {
      ...parsed,
      url,
      host: citationHost(url),
      number: sources.size + 1,
    });
  }
  return [...sources.values()];
}

/** Splits the link title into the source's title and the supporting passage. */
export function parseCitationTitle(
  title: string | undefined,
  url: string,
): CitationText {
  const text = title?.trim();
  if (!text) return { title: citationHost(url) };
  const separator = SNIPPET_SEPARATOR.exec(text);
  if (!separator) return { title: text };
  const head = text.slice(0, separator.index).trim();
  const snippet = text.slice(separator.index + separator[0].length).trim();
  return {
    title: head || citationHost(url),
    ...(snippet ? { snippet } : {}),
  };
}
