/**
 * Finds and paints the claim a citation chip backs: the sentence before it,
 * back to the previous sentence end or the previous chip. It is read from the
 * rendered DOM, so the Markdown stays plain.
 */

export const CLAIM_HIGHLIGHT = "monocode-citation-claim";

/** Paints the claim with the accent; Tailwind cannot name a highlight pseudo, so it goes through a variant. */
export const CLAIM_HIGHLIGHT_CLASS = [
  `[&_*::highlight(${CLAIM_HIGHLIGHT})]:bg-accent/15`,
  `[&_*::highlight(${CLAIM_HIGHLIGHT})]:underline`,
  `[&_*::highlight(${CLAIM_HIGHLIGHT})]:decoration-accent/60`,
].join(" ");

const CHIP = "[data-citation]";
const BLOCK = "p, li, td, th, h1, h2, h3, h4, h5, h6, blockquote, dd, dt";
const FOLLOWING = 4; // Node.DOCUMENT_POSITION_FOLLOWING

function textNodes(root: Node, out: Text[] = []): Text[] {
  for (const child of Array.from(root.childNodes)) {
    if (child.nodeType === 3) out.push(child as Text);
    else if (child.nodeType === 1 && !(child as Element).matches(CHIP)) {
      textNodes(child, out);
    }
  }
  return out;
}

const follows = (a: Node, b: Node) =>
  !!(a.compareDocumentPosition(b) & FOLLOWING);

export function claimRange(chip: HTMLElement): Range | null {
  const block = chip.closest(BLOCK);
  if (!block) return null;
  const chips = Array.from(block.querySelectorAll<HTMLElement>(CHIP));
  const nodes = textNodes(block);
  let first = chips.indexOf(chip);
  if (first < 0) return null;
  // Chips that touch back a single claim; the claim ends before the first.
  while (
    first > 0 &&
    nodes
      .filter((node) => follows(chips[first - 1], node) && follows(node, chips[first]))
      .every((node) => !node.data.trim())
  ) {
    first -= 1;
  }
  const previous = first > 0 ? chips[first - 1] : null;
  const eligible = nodes.filter(
    (node) =>
      follows(node, chips[first]) && (!previous || follows(previous, node)),
  );
  if (eligible.length === 0) return null;

  const text = eligible.map((node) => node.data).join("");
  let end = text.length;
  while (end > 0 && /[\s.!?…:;]/.test(text[end - 1])) end -= 1;
  let start = 0;
  for (const boundary of text.slice(0, end).matchAll(/[.!?…]["')\]]*\s+/g)) {
    start = boundary.index + boundary[0].length;
  }
  while (start < end && /[\s,;:)]/.test(text[start])) start += 1;
  if (start >= end) return null;

  const locate = (position: number): [Text, number] => {
    let seen = 0;
    for (const node of eligible) {
      if (position <= seen + node.length) return [node, position - seen];
      seen += node.length;
    }
    const last = eligible[eligible.length - 1];
    return [last, last.length];
  };
  const range = document.createRange();
  range.setStart(...locate(start));
  range.setEnd(...locate(end));
  return range;
}

export function paintClaims(ranges: Range[]): void {
  if (typeof CSS === "undefined" || !("highlights" in CSS)) return;
  if (ranges.length === 0) CSS.highlights.delete(CLAIM_HIGHLIGHT);
  else CSS.highlights.set(CLAIM_HIGHLIGHT, new Highlight(...ranges));
}
