import { arxivId, normalizeDoi } from "./papers";
import { paperKeyOfSource } from "./paperSources";
import { paperKnown } from "./paperStore";

/** Asks the app to show a paper it has read, at a cited passage. */
export type OpenPaperRequest = {
  key: string;
  /** The sentence a citation quotes: the viewer underlines it. */
  passage?: string;
};

export const OPEN_PAPER_EVENT = "monobot:open-paper";
export const FOCUS_PAPER_EVENT = "monobot:focus-paper";

/** Which papers a viewer is showing right now, and how many viewers each. */
const shown = new Map<string, number>();

/** A viewer announces the paper it shows; the returned function withdraws it. */
export function announcePaperShown(key: string): () => void {
  shown.set(key, (shown.get(key) ?? 0) + 1);
  return () => {
    const left = (shown.get(key) ?? 1) - 1;
    if (left > 0) shown.set(key, left);
    else shown.delete(key);
  };
}

export function isPaperShown(key: string): boolean {
  return shown.has(key);
}

/** What a citation asks of a viewer already showing its paper. */
export type FocusPaperRequest = { key: string; passage?: string };

/**
 * Points the viewer that already shows a cited paper at the cited passage, so
 * the answer in the chat and the paper beside it speak about the same place.
 * False when no viewer shows that paper (the caller then opens it itself).
 */
export function focusPaperPassage(href: string, passage?: string): boolean {
  const key = paperKeyOf(href);
  if (!key || !isPaperShown(key)) return false;
  window.dispatchEvent(
    new CustomEvent<FocusPaperRequest>(FOCUS_PAPER_EVENT, {
      detail: { key, ...(passage ? { passage } : {}) },
    }),
  );
  return true;
}

export function openPaper(request: OpenPaperRequest) {
  window.dispatchEvent(
    new CustomEvent<OpenPaperRequest>(OPEN_PAPER_EVENT, { detail: request }),
  );
}

/**
 * The cache key a citation's link names: its DOI, the arXiv DOI, or the PDF
 * without a DOI that the app downloaded from that link.
 */
export function paperKeyOf(href: string): string | undefined {
  const doi = normalizeDoi(href);
  if (doi) return doi;
  const arxiv = arxivId(href);
  if (arxiv) return `10.48550/arxiv.${arxiv.toLowerCase()}`;
  return paperKeyOfSource(href);
}

/**
 * A click on a citation whose source is a paper the app has read opens it here
 * (true); any other source is left to the browser (false).
 */
export async function openCitedPaper(
  href: string,
  passage?: string,
): Promise<boolean> {
  const key = paperKeyOf(href);
  if (!key || !(await paperKnown(key))) return false;
  openPaper({ key, ...(passage ? { passage } : {}) });
  return true;
}

/**
 * What clicking a cited source does: a paper the app has read opens in its
 * viewer, underlined at the quoted passage; any other source opens in the
 * browser through `fallback`.
 */
export async function openSource(
  href: string,
  passage: string | undefined,
  fallback: (href: string) => Promise<unknown>,
): Promise<void> {
  // A paper already open beside the chat: show the passage there.
  if (focusPaperPassage(href, passage)) return;
  const opened = await openCitedPaper(href, passage).catch(() => false);
  if (!opened) await fallback(href);
}
