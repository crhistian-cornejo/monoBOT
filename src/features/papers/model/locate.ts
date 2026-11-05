/**
 * Finds a cited passage inside a paper's text, to underline it. An agent quotes
 * the paper, but the PDF's text differs in small ways: words broken across
 * lines with a hyphen, ligatures ("ﬁ"), curly quotes, a stray space. So both
 * sides are reduced to their letters and digits before they are compared, and
 * a hit is mapped back to where it sits in the original text.
 */

/** A span of text: item `startItem` from `startOffset`, to `endItem` before `endOffset`. */
export type TextRange = {
  startItem: number;
  startOffset: number;
  endItem: number;
  endOffset: number;
};

export type Located = TextRange & {
  /** How much of the passage was found, 0 to 1. 1 is the whole sentence. */
  coverage: number;
};

/** The shortest passage worth underlining: less matches anywhere. */
const MIN_CHARS = 18;

type Folded = {
  text: string;
  /** For each folded character, the item and offset it came from. */
  at: { item: number; offset: number }[];
};

function fold(items: readonly string[]): Folded {
  let text = "";
  const at: Folded["at"] = [];
  items.forEach((item, index) => {
    let offset = 0;
    for (const char of item) {
      for (const kept of char.normalize("NFKC").toLowerCase()) {
        if (/[\p{L}\p{N}]/u.test(kept)) {
          text += kept;
          at.push({ item: index, offset });
        }
      }
      offset += char.length;
    }
  });
  return { text, at };
}

function foldText(text: string): string {
  return fold([text]).text;
}

/** The longest prefix of `needle` that `haystack` holds, and where. */
function longestPrefix(
  haystack: string,
  needle: string,
  floor: number,
): { length: number; index: number } | undefined {
  for (let length = needle.length; length >= floor; length -= 4) {
    const index = haystack.indexOf(needle.slice(0, length));
    if (index >= 0) return { length, index };
  }
  return undefined;
}

function longestSuffix(
  haystack: string,
  needle: string,
  floor: number,
): { length: number; index: number } | undefined {
  for (let length = needle.length; length >= floor; length -= 4) {
    const index = haystack.indexOf(needle.slice(needle.length - length));
    if (index >= 0) return { length, index };
  }
  return undefined;
}

function toRange(
  folded: Folded,
  from: number,
  length: number,
  coverage: number,
): Located {
  const first = folded.at[from];
  const last = folded.at[from + length - 1];
  return {
    startItem: first.item,
    startOffset: first.offset,
    endItem: last.item,
    // One past the last character, in the item's own string.
    endOffset: last.offset + 1,
    coverage,
  };
}

/**
 * Where `passage` sits in `items` (a page's text pieces in reading order), or
 * undefined. A whole-sentence match wins; otherwise the part of it that is
 * there, when that part is a good share of it.
 */
export function locateInItems(
  items: readonly string[],
  passage: string,
): Located | undefined {
  const wanted = foldText(passage);
  if (wanted.length < MIN_CHARS) return undefined;
  const folded = fold(items);
  if (folded.text.length < MIN_CHARS) return undefined;
  const whole = folded.text.indexOf(wanted);
  if (whole >= 0) return toRange(folded, whole, wanted.length, 1);

  // Both ends of the sentence are there, with its middle between them.
  const edge = Math.min(24, Math.floor(wanted.length / 2));
  const head = folded.text.indexOf(wanted.slice(0, edge));
  if (head >= 0) {
    const tail = folded.text.indexOf(wanted.slice(-edge), head + edge);
    if (tail >= 0 && tail + edge - head <= wanted.length * 1.5 + 40)
      return toRange(folded, head, tail + edge - head, 0.9);
  }

  // Otherwise the longest piece of it that is in the page.
  const floor = Math.max(MIN_CHARS, Math.ceil(wanted.length * 0.4));
  const start = longestPrefix(folded.text, wanted, floor);
  const end = longestSuffix(folded.text, wanted, floor);
  const best =
    start && end ? (start.length >= end.length ? start : end) : (start ?? end);
  if (!best) return undefined;
  return toRange(folded, best.index, best.length, best.length / wanted.length);
}

/**
 * The page a passage is on, from the text the paper was read into. `hint` is
 * a page the agent named, tried first.
 */
export function locatePage(
  pages: readonly string[],
  passage: string,
  hint?: number,
): { page: number; coverage: number } | undefined {
  let best: { page: number; coverage: number } | undefined;
  const order = pages.map((_, index) => index + 1);
  if (hint && hint >= 1 && hint <= pages.length)
    order.sort((a, b) => (a === hint ? -1 : b === hint ? 1 : a - b));
  for (const page of order) {
    const found = locateInItems([pages[page - 1]], passage);
    if (!found) continue;
    if (found.coverage === 1) return { page, coverage: 1 };
    if (!best || found.coverage > best.coverage)
      best = { page, coverage: found.coverage };
  }
  return best;
}

/** The pieces of a located range, item by item, for drawing it. */
export function rangeParts(
  items: readonly string[],
  range: TextRange,
): { item: number; from: number; to: number }[] {
  const parts: { item: number; from: number; to: number }[] = [];
  for (let item = range.startItem; item <= range.endItem; item += 1) {
    const length = items[item]?.length ?? 0;
    const from = item === range.startItem ? range.startOffset : 0;
    const to = item === range.endItem ? range.endOffset : length;
    if (to > from) parts.push({ item, from, to });
  }
  return parts;
}
