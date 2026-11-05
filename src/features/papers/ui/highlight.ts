import { rangeParts, type TextRange } from "../model/locate";

const NAME = "paper-cite";
const FOCUS = "paper-cite-focus";

type HighlightRegistry = {
  set(name: string, highlight: unknown): void;
  delete(name: string): void;
};

function registry(): HighlightRegistry | undefined {
  const css = (globalThis as { CSS?: { highlights?: HighlightRegistry } }).CSS;
  return css?.highlights && typeof Highlight === "function"
    ? css.highlights
    : undefined;
}

/**
 * What each page has underlined. The browser keeps one highlight per name, so
 * every page adds its ranges here and the whole is what gets drawn: a page
 * drawing after another must not erase it.
 */
const byOwner = new Map<object, Range[]>();

function publish() {
  const highlights = registry();
  if (!highlights) return;
  const all = [...byOwner.values()].flat();
  if (all.length) highlights.set(NAME, new Highlight(...all));
  else highlights.delete(NAME);
}

/** Removes what `owner` (a page's text layer) underlined. */
export function clearHighlight(
  owner: object,
  divs: readonly HTMLElement[] = [],
) {
  byOwner.delete(owner);
  publish();
  for (const div of divs) div.classList.remove("paper-cite-span");
}

/**
 * Underlines located ranges over a page's text pieces. The browser's highlight
 * registry draws exactly the quoted words; without it, the lines that hold them
 * are marked. Returns, for each input range, the DOM range that covers it, so
 * the caller can scroll to it later (its position changes as the page moves).
 */
export function drawHighlights(
  owner: object,
  divs: readonly HTMLElement[],
  located: readonly TextRange[],
): (Range | undefined)[] {
  const items = divs.map((div) => div.textContent ?? "");
  const highlights = registry();
  const all: Range[] = [];
  const covering = located.map((range) => {
    let first: Range | undefined;
    let last: Range | undefined;
    for (const { item, from, to } of rangeParts(items, range)) {
      const div = divs[item];
      const node = div?.firstChild;
      if (!div || !node || node.nodeType !== Node.TEXT_NODE) continue;
      const length = node.textContent?.length ?? 0;
      const piece = document.createRange();
      piece.setStart(node, Math.min(from, length));
      piece.setEnd(node, Math.min(to, length));
      all.push(piece);
      first ??= piece;
      last = piece;
      if (!highlights) div.classList.add("paper-cite-span");
    }
    if (!first || !last) return undefined;
    const whole = document.createRange();
    whole.setStart(first.startContainer, first.startOffset);
    whole.setEnd(last.endContainer, last.endOffset);
    return whole;
  });
  byOwner.set(owner, all);
  publish();
  return covering;
}

/**
 * Marks one passage more strongly than the rest: the one the chat is talking
 * about right now. Pass nothing to take the emphasis off.
 */
export function focusHighlight(range?: Range) {
  const highlights = registry();
  if (!highlights) return;
  if (range) highlights.set(FOCUS, new Highlight(range));
  else highlights.delete(FOCUS);
}
