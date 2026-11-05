/*
 * How a streamed reply's words come out and come into focus. The numbers are
 * the ones the CSS in index.css uses (`word-fade-in`, `word-trail`); the
 * functions decide how far into the text the screen may go.
 */

/** How long a word takes to come into focus (opacity and a small blur). */
export const WORD_FOCUS_MS = 560;
/** How long a word keeps a trail of the accent colour before it is text-coloured. */
export const WORD_TRAIL_MS = 1120;
/** The curve both run on: quick to start, long to settle. */
export const WORD_EASE = "cubic-bezier(0.22, 1, 0.36, 1)";
/** Words let out in the same paint start this far apart. */
export const WORD_STAGGER_MS = 20;
/** The most a paced word waits behind the others of its paint. */
export const PACED_STAGGER_CAP_MS = 60;

/** How long the word spans must stay once nothing more comes out: the longest of the fade and the trail. */
export function wordFadeLingerMs(fadeMs: number): number {
  return Math.max(fadeMs, WORD_TRAIL_MS);
}

/** When the word at `index` of one paint starts fading in. */
export function wordStaggerMs(index: number, capMs: number): number {
  return Math.max(0, Math.min(index * WORD_STAGGER_MS, capMs));
}

function isSpace(code: number): boolean {
  return code === 32 || code === 10 || code === 9 || code === 13;
}

/**
 * A word that reads on its own. A Markdown mark alone (a list's `-`, `##`,
 * `|`, `>`, `1.`) goes out with the word after it, so a bullet or a table bar
 * never sits on screen waiting for its text.
 */
export function wordReads(word: string): boolean {
  return /[\p{L}\p{N}]/u.test(word) && !/^\d+[.)]$/.test(word);
}

/** Where the word that starts at or before `from` begins. */
function wordStart(text: string, from: number): number {
  let start = from;
  while (start > 0 && !isSpace(text.charCodeAt(start - 1))) start--;
  return start;
}

/**
 * Where a stream still being written may stop showing `text`: before the word
 * still being written, and before any marks waiting for the word after them.
 */
function heldEnd(text: string): number {
  let end = wordStart(text, text.length);
  for (;;) {
    let wordEnd = end;
    while (wordEnd > 0 && isSpace(text.charCodeAt(wordEnd - 1))) wordEnd--;
    const start = wordStart(text, wordEnd);
    if (start === wordEnd || wordReads(text.slice(start, wordEnd))) return end;
    end = start;
  }
}

/**
 * Where to stop revealing `text` for a reveal that has reached `at`: the end
 * of the word `at` falls in, so a word is never shown half written, and past
 * any mark it lands on to the end of the word after it. A stream still
 * mid-word holds back at the last whole word; a finished one runs out.
 */
export function revealEnd(
  text: string,
  at: number,
  streaming: boolean,
): number {
  let i = Math.max(0, Math.ceil(at));
  while (i < text.length) {
    while (i < text.length && !isSpace(text.charCodeAt(i))) i++;
    if (i >= text.length) break;
    if (wordReads(text.slice(wordStart(text, i), i))) return i;
    while (i < text.length && isSpace(text.charCodeAt(i))) i++;
  }
  if (!streaming) return text.length;
  return heldEnd(text);
}

const TABLE_LINE = /^ {0,3}\|/;
const DELIMITER_CELL = /^\s*:?-+:?\s*$/;

/** The cells of a table row, without the bars at its ends. */
function tableCells(line: string): string[] {
  const inner = line
    .trim()
    .replace(/^\|/, "")
    .replace(/(?<!\\)\|$/, "");
  return inner.split(/(?<!\\)\|/);
}

function isDelimiterRow(line: string, header: string): boolean {
  const cells = tableCells(line);
  return (
    cells.length === tableCells(header).length &&
    cells.every((cell) => DELIMITER_CELL.test(cell))
  );
}

/**
 * `text` without a table it ends in that is not a table yet: its rows wait
 * until the header and the line under it have arrived, so they never show as
 * a paragraph of bars first and then jump into a table. Only the last lines
 * are looked at unless they start with a bar, so a long reply costs nothing
 * extra. Inside an open code fence nothing is held.
 */
export function holdHalfTable(text: string): string {
  const body = text.endsWith("\n") ? text.slice(0, -1) : text;
  let start = body.length + 1;
  let end = body.length;
  const lines: string[] = [];
  for (;;) {
    const from = end === 0 ? 0 : body.lastIndexOf("\n", end - 1) + 1;
    const line = body.slice(from, end);
    if (!TABLE_LINE.test(line)) break;
    lines.unshift(line);
    start = from;
    if (from === 0) break;
    end = from - 1;
  }
  if (!lines.length) return text;
  if ((text.match(/^ {0,3}(?:`{3,}|~{3,})/gm)?.length ?? 0) % 2) return text;
  if (lines.length >= 2 && isDelimiterRow(lines[1]!, lines[0]!)) return text;
  return text.slice(0, start);
}
