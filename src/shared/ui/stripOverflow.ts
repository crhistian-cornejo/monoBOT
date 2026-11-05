export type StripOverflow = { left: boolean; right: boolean };

/** Which edges of a horizontal strip still have content to scroll toward. */
export function stripOverflow(
  scrollLeft: number,
  clientWidth: number,
  scrollWidth: number,
): StripOverflow {
  const maxScroll = scrollWidth - clientWidth;
  if (maxScroll <= 1) return { left: false, right: false };
  return {
    left: scrollLeft > 1,
    right: scrollLeft < maxScroll - 1,
  };
}

/**
 * The scroll position that shows an item whole, moving as little as possible.
 * `margin` keeps the item clear of the faded edges. The first item scrolls the
 * strip back to its very start.
 */
export function revealScrollLeft(
  itemLeft: number,
  itemWidth: number,
  scrollLeft: number,
  clientWidth: number,
  margin = 0,
): number {
  if (itemLeft - margin <= 0) return 0;
  if (itemLeft - margin < scrollLeft) return itemLeft - margin;
  const itemRight = itemLeft + itemWidth + margin;
  if (itemRight > scrollLeft + clientWidth) return itemRight - clientWidth;
  return scrollLeft;
}
