import { paperKeyOf } from "./openPaper";

/**
 * The passages an agent's answers have cited from each paper, kept on this
 * computer so the paper can be shown with all of them underlined, not only the
 * one a click named. A citation records itself when it is shown.
 */

const STORE = "monocode.paperPassages";
const PER_PAPER = 40;
const PAPERS = 60;
/** Shorter than this matches anywhere, so it is not worth underlining. */
const MIN_CHARS = 24;

/** A cited passage, and the number the answer gave the paper in its citations. */
type Entry = { t: string; n?: number };
type Passages = Record<string, (Entry | string)[]>;

export type CitedPassage = {
  passage: string;
  /** The citation's number in the answer that cited it: "1" in [1]. */
  number?: number;
};

function read(): Passages {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STORE) ?? "{}");
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Passages)
      : {};
  } catch {
    return {};
  }
}

function write(all: Passages) {
  try {
    localStorage.setItem(STORE, JSON.stringify(all));
  } catch {
    // Private mode or a full disk: the underline then only follows clicks.
  }
}

/** Passages saved before numbers were kept are plain strings. */
function entries(list: Passages[string] | undefined): Entry[] {
  if (!Array.isArray(list)) return [];
  return list.flatMap((entry) => {
    if (typeof entry === "string") return [{ t: entry }];
    return entry && typeof entry.t === "string"
      ? [
          {
            t: entry.t,
            ...(Number.isInteger(entry.n) ? { n: entry.n } : {}),
          },
        ]
      : [];
  });
}

/**
 * Notes that an answer cited `passage` from the paper `href` points to, as
 * citation `number` there (the "1" of [1]).
 */
export function recordPaperPassage(
  href: string,
  passage: string | undefined,
  number?: number,
) {
  const text = passage?.trim();
  if (!text || text.length < MIN_CHARS) return;
  const key = paperKeyOf(href);
  if (!key) return;
  const all = read();
  const known = entries(all[key]);
  const same = known.find((entry) => entry.t === text);
  // Already kept, with the number it has now: nothing to write on every render.
  if (same && (number === undefined || same.n === number)) return;
  const kept = same
    ? known.map((entry) => (entry === same ? { ...entry, n: number } : entry))
    : [...known, { t: text, ...(number !== undefined ? { n: number } : {}) }];
  // Re-insert the paper last, so the least recently cited ones are dropped.
  delete all[key];
  all[key] = kept.slice(-PER_PAPER);
  for (const old of Object.keys(all).slice(
    0,
    Math.max(0, Object.keys(all).length - PAPERS),
  ))
    delete all[old];
  write(all);
}

/** Every passage cited from this paper so far, with its citation number, oldest first. */
export function paperCitations(key: string): CitedPassage[] {
  return entries(read()[key]).map((entry) => ({
    passage: entry.t,
    ...(entry.n !== undefined ? { number: entry.n } : {}),
  }));
}

/** Every passage cited from this paper so far, oldest first. */
export function paperPassages(key: string): string[] {
  return paperCitations(key).map((entry) => entry.passage);
}

/** How a cited passage is named in words: "citation 1, page 3". */
export function citedLabel(number?: number, page?: number): string {
  return [
    number !== undefined ? `citation ${number}` : "",
    page !== undefined ? `page ${page}` : "",
  ]
    .filter(Boolean)
    .join(", ");
}

/**
 * Where a saved copy sits, in words: "Documents / MonoBOT / Research", from a
 * path on either macOS or Windows, and the file's own name.
 */
export function savedLocation(path: string): { folder: string; file: string } {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  const file = parts.pop() ?? path;
  const from = parts.lastIndexOf("Documents");
  const shown = from >= 0 ? parts.slice(from) : parts.slice(-2);
  return { folder: shown.join(" / "), file };
}
