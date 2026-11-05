import { describe, expect, it } from "vitest";
import { cropAround, partBoxes, unionBox } from "./passageCrop";

const page = { width: 600, height: 800 };

describe("partBoxes", () => {
  const items = [
    { box: { x: 50, y: 100, width: 200, height: 10 }, length: 20 },
    { box: { x: 50, y: 112, width: 220, height: 10 }, length: 22 },
  ];

  it("takes the share of an item the passage covers", () => {
    expect(
      partBoxes(items, [
        { item: 0, from: 10, to: 20 },
        { item: 1, from: 0, to: 11 },
      ]),
    ).toEqual([
      { x: 150, y: 100, width: 100, height: 10 },
      { x: 50, y: 112, width: 110, height: 10 },
    ]);
  });

  it("skips items it does not know", () => {
    expect(partBoxes(items, [{ item: 5, from: 0, to: 3 }])).toEqual([]);
  });
});

describe("unionBox", () => {
  it("covers every box", () => {
    expect(
      unionBox([
        { x: 150, y: 100, width: 100, height: 10 },
        { x: 50, y: 112, width: 110, height: 10 },
      ]),
    ).toEqual({ x: 50, y: 100, width: 200, height: 22 });
  });
});

describe("cropAround", () => {
  it("keeps to the column of a two-column paper", () => {
    const crop = cropAround([{ x: 320, y: 300, width: 240, height: 30 }], page);
    expect(crop).toEqual({ x: 302, y: 282, width: 276, height: 66 });
  });

  it("widens a short phrase and keeps it on the page", () => {
    const crop = cropAround([{ x: 560, y: 10, width: 30, height: 10 }], page);
    expect(crop).toEqual({ x: 380, y: 0, width: 220, height: 38 });
  });

  it("spans the page for a passage across it and caps its height", () => {
    const crop = cropAround(
      [{ x: 40, y: 200, width: 520, height: 400 }],
      page,
      { maxHeight: 150 },
    );
    expect(crop).toEqual({ x: 0, y: 182, width: 600, height: 150 });
  });

  it("gives nothing without a passage", () => {
    expect(cropAround([], page)).toBeUndefined();
  });
});
