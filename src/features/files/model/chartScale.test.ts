import { describe, expect, it } from "vitest";
import { niceScale, stackedSegments, ticks, valueRange } from "./chartScale";
import type { ChartSeries } from "./spreadsheetDrawings";

const series = (values: (number | null)[]): ChartSeries => ({
  kind: "column",
  name: "s",
  values,
});

describe("niceScale", () => {
  it("rounds bounds to whole steps", () => {
    expect(niceScale(0, 340000)).toEqual({ min: 0, max: 400000, step: 100000 });
    expect(niceScale(12, 87)).toEqual({ min: 0, max: 100, step: 20 });
    expect(ticks(niceScale(-3, 7))).toEqual([-4, -2, 0, 2, 4, 6, 8]);
  });

  it("gives a flat series an axis from zero", () => {
    expect(niceScale(5, 5).min).toBe(0);
    expect(niceScale(0, 0)).toEqual({ min: 0, max: 1, step: 0.2 });
  });
});

describe("stackedSegments", () => {
  it("stacks values and turns percent stacks into shares", () => {
    const pair = [series([1, 2]), series([3, null])];
    expect(stackedSegments(pair, "stacked", 2)).toEqual([
      [
        { from: 0, to: 1 },
        { from: 0, to: 2 },
      ],
      [
        { from: 1, to: 4 },
        { from: 2, to: 2 },
      ],
    ]);
    const percent = stackedSegments(pair, "percent", 2);
    expect(percent[1][0]).toEqual({ from: 25, to: 100 });
    expect(valueRange(percent, true)).toEqual([0, 100]);
  });
});
