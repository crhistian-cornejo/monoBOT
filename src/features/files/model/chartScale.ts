import type { ChartSeries, SheetChart } from "./spreadsheetDrawings";

/** An axis from `min` to `max` in round steps, as a chart labels it. */
export type Scale = { min: number; max: number; step: number };

/** Round bounds and a 1, 2, 2.5 or 5 step that covers the values in about `count` ticks. */
export function niceScale(low: number, high: number, count = 5): Scale {
  if (!Number.isFinite(low) || !Number.isFinite(high))
    return { min: 0, max: 1, step: 0.2 };
  // A flat series still gets an axis: from zero up to it, or down to it.
  if (low === high) {
    if (low === 0) high = 1;
    else if (low > 0) low = 0;
    else high = 0;
  }
  const raw = (high - low) / Math.max(1, count);
  const power = 10 ** Math.floor(Math.log10(raw));
  const step =
    [1, 2, 2.5, 5, 10]
      .map((factor) => factor * power)
      .find((candidate) => candidate >= raw) ?? 10 * power;
  const min = Math.floor(low / step + 1e-9) * step;
  const max = Math.ceil(high / step - 1e-9) * step;
  return { min, max: max === min ? min + step : max, step };
}

export function ticks(scale: Scale): number[] {
  const out: number[] = [];
  for (
    let value = scale.min;
    value <= scale.max + scale.step / 1e6;
    value += scale.step
  )
    out.push(Number(value.toPrecision(12)));
  return out;
}

/** Short axis labels: 1 500, 12.5, 2.4M. */
export function axisLabel(value: number, locale?: string): string {
  const compact = Math.abs(value) >= 1e5;
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: compact ? 1 : 2,
    notation: compact ? "compact" : "standard",
  }).format(value);
}

/**
 * The values each series draws, after stacking: per point, where its segment
 * starts and ends. Percent stacking turns each category into shares of 100.
 */
export function stackedSegments(
  series: readonly ChartSeries[],
  stacking: SheetChart["stacking"],
  count: number,
): { from: number; to: number }[][] {
  if (!stacking)
    return series.map((entry) =>
      Array.from({ length: count }, (_, index) => ({
        from: 0,
        to: entry.values[index] ?? 0,
      })),
    );
  const totals = Array.from({ length: count }, (_, index) =>
    series.reduce((sum, entry) => sum + Math.abs(entry.values[index] ?? 0), 0),
  );
  const up = Array.from({ length: count }, () => 0);
  const down = Array.from({ length: count }, () => 0);
  return series.map((entry) =>
    Array.from({ length: count }, (_, index) => {
      let value = entry.values[index] ?? 0;
      if (stacking === "percent")
        value = totals[index] ? (value / totals[index]) * 100 : 0;
      const base = value >= 0 ? up : down;
      const from = base[index];
      base[index] += value;
      return { from, to: base[index] };
    }),
  );
}

/** The lowest and highest values a chart's value axis must show. */
export function valueRange(
  segments: { from: number; to: number }[][],
  includeZero: boolean,
): [number, number] {
  let low = includeZero ? 0 : Infinity;
  let high = includeZero ? 0 : -Infinity;
  for (const points of segments)
    for (const { to } of points) {
      low = Math.min(low, to);
      high = Math.max(high, to);
    }
  return Number.isFinite(low) ? [low, high] : [0, 1];
}
