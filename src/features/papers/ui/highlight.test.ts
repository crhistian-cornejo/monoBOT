// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearHighlight, drawHighlights } from "./highlight";

/** A stand-in for the browser's highlight registry, which one name shares. */
class FakeHighlight {
  readonly ranges: Range[];
  constructor(...ranges: Range[]) {
    this.ranges = ranges;
  }
}
const registry = new Map<string, FakeHighlight>();

beforeEach(() => {
  registry.clear();
  vi.stubGlobal("Highlight", FakeHighlight);
  vi.stubGlobal("CSS", {
    highlights: {
      set: (name: string, value: FakeHighlight) => registry.set(name, value),
      delete: (name: string) => registry.delete(name),
    },
  });
});

/** Pages a test drew, cleared after it: the module remembers them. */
const drawn: HTMLElement[] = [];
afterEach(() => {
  for (const layer of drawn.splice(0)) clearHighlight(layer);
});

function page(...lines: string[]) {
  const layer = document.createElement("div");
  drawn.push(layer);
  const divs = lines.map((line) => {
    const span = document.createElement("span");
    span.textContent = line;
    layer.append(span);
    return span;
  });
  return { layer, divs };
}

const whole = (item: number, length: number) => ({
  startItem: item,
  startOffset: 0,
  endItem: item,
  endOffset: length,
});

describe("highlights across pages", () => {
  it("keeps one page's underline when another page draws its own", () => {
    const one = page("The first cited sentence.");
    const two = page("The second cited sentence.");
    drawHighlights(one.layer, one.divs, [whole(0, 25)]);
    drawHighlights(two.layer, two.divs, [whole(0, 26)]);
    const kept = registry
      .get("paper-cite")!
      .ranges.map((range) => range.toString());
    expect(kept).toEqual([
      "The first cited sentence.",
      "The second cited sentence.",
    ]);
  });

  it("removes only what the leaving page drew, and the name when none is left", () => {
    const one = page("The first cited sentence.");
    const two = page("The second cited sentence.");
    drawHighlights(one.layer, one.divs, [whole(0, 25)]);
    drawHighlights(two.layer, two.divs, [whole(0, 26)]);
    clearHighlight(one.layer, one.divs);
    expect(registry.get("paper-cite")!.ranges.map((r) => r.toString())).toEqual(
      ["The second cited sentence."],
    );
    clearHighlight(two.layer, two.divs);
    expect(registry.has("paper-cite")).toBe(false);
  });

  it("returns the range of each passage, over the words it spans", () => {
    const { layer, divs } = page(
      "Calibration of the GR4J ",
      "model works well here.",
    );
    const [covering] = drawHighlights(layer, divs, [
      { startItem: 0, startOffset: 19, endItem: 1, endOffset: 11 },
    ]);
    expect(covering?.toString()).toBe("GR4J model works");
  });

  it("marks the lines instead when the browser has no highlight registry", () => {
    vi.stubGlobal("CSS", {});
    const { layer, divs } = page("A cited sentence on one line.");
    drawHighlights(layer, divs, [whole(0, 29)]);
    expect(divs[0].classList.contains("paper-cite-span")).toBe(true);
    clearHighlight(layer, divs);
    expect(divs[0].classList.contains("paper-cite-span")).toBe(false);
  });
});
