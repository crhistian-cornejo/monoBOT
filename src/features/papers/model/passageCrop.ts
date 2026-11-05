/**
 * Where a cited passage sits on its page, and the part of the page to show
 * around it in a citation's preview. Boxes are in page units with the origin
 * at the top left, as pdf.js lays a page's text out at scale 1.
 */

export type Box = { x: number; y: number; width: number; height: number };

/** One text item of a page: its box and how many characters it holds. */
export type ItemBox = { box: Box; length: number };

/**
 * The boxes of a located passage, one per text item it touches. An item only
 * partly in the passage gives the share of its width its characters take.
 */
export function partBoxes(
  items: readonly ItemBox[],
  parts: readonly { item: number; from: number; to: number }[],
): Box[] {
  return parts.flatMap(({ item, from, to }) => {
    const entry = items[item];
    if (!entry || entry.length <= 0 || entry.box.width <= 0) return [];
    const { box, length } = entry;
    const start = (Math.max(0, from) / length) * box.width;
    const end = (Math.min(length, to) / length) * box.width;
    if (end <= start) return [];
    return [{ x: box.x + start, y: box.y, width: end - start, height: box.height }];
  });
}

export function unionBox(boxes: readonly Box[]): Box | undefined {
  if (!boxes.length) return undefined;
  const left = Math.min(...boxes.map((box) => box.x));
  const top = Math.min(...boxes.map((box) => box.y));
  const right = Math.max(...boxes.map((box) => box.x + box.width));
  const bottom = Math.max(...boxes.map((box) => box.y + box.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

export type CropOptions = {
  /** Space kept around the passage, in page units. */
  pad?: number;
  /** The tallest crop; a longer passage shows from its start. */
  maxHeight?: number;
  /** The narrowest crop, so a short phrase still shows its line. */
  minWidth?: number;
};

/**
 * The part of the page to show: the passage with some of the text around it.
 * A passage in one column of a two-column paper keeps to that column, so the
 * crop is not mostly the other one.
 */
export function cropAround(
  marks: readonly Box[],
  page: { width: number; height: number },
  { pad = 18, maxHeight = 220, minWidth = 220 }: CropOptions = {},
): Box | undefined {
  const passage = unionBox(marks);
  if (!passage) return undefined;
  const clamp = (value: number, low: number, high: number) =>
    Math.min(Math.max(value, low), high);
  const oneColumn = passage.width < page.width * 0.62;
  let left = oneColumn ? passage.x - pad : 0;
  let right = oneColumn ? passage.x + passage.width + pad : page.width;
  const width = Math.min(page.width, Math.max(right - left, minWidth));
  const middle = (left + right) / 2;
  left = clamp(middle - width / 2, 0, page.width - width);
  right = left + width;
  const top = clamp(passage.y - pad, 0, page.height);
  const bottom = clamp(
    Math.min(passage.y + passage.height + pad, top + maxHeight),
    top,
    page.height,
  );
  return { x: left, y: top, width: right - left, height: bottom - top };
}
