// The native app's dithered charts (crates/monocode-gpui/src/visuals/charts.rs),
// drawn with SVG in the chat: the same 3px cells, tones and curves. Line and
// area charts take dithercn's scrub readout: a crosshair that follows the
// pointer or the arrow keys, a legend that spotlights a series, and the data
// in a hidden table for screen readers.
import {
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";

import { CELL, cells, center, inked, step } from "../model/dither";
import { Curve, compact, Scale } from "../model/scale";
import type { Series, Slice, ThemeColor } from "../model/visual";

/** A theme token as a CSS colour; series 1 is the user's accent. */
export function visualColor(token: ThemeColor | undefined = "accent"): string {
  switch (token) {
    case "ink":
      return "var(--color-content)";
    case "muted":
      return mix("var(--color-content)", 60);
    case "faint":
      return mix("var(--color-content)", 20);
    case "positive":
    case "series2":
      return "var(--color-visual-positive)";
    case "negative":
      return "var(--color-visual-negative)";
    case "warning":
    case "series4":
      return "var(--color-visual-warning)";
    case "series3":
      return "var(--color-visual-series3)";
    case "series5":
      return "var(--color-visual-series5)";
    case "series6":
      return "var(--color-visual-series6)";
    default:
      return "var(--color-accent)";
  }
}

/** `color` at `percent` strength over whatever is behind it. */
export const mix = (color: string, percent: number) =>
  `color-mix(in srgb, ${color} ${percent}%, transparent)`;

const AXIS_STYLE = {
  fill: mix("var(--color-content)", 45),
  fontSize: 10,
  fontFamily: "var(--font-mono)",
} as const;

/** The width its box gets, once laid out. */
export function useWidth<T extends HTMLElement>() {
  const ref = useRef<T>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const update = () => setWidth(Math.floor(element.clientWidth));
    update();
    const observer = new ResizeObserver(update);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return [ref, width] as const;
}

/** One `<path>` per colour of a cell grid, offset to (x, y). */
function Cells({
  paths,
  x = 0,
  y = 0,
  dim,
}: {
  paths: Map<string, string>;
  x?: number;
  y?: number;
  /** Colours drawn faint, when a series is spotlit. */
  dim?: (color: string) => boolean;
}) {
  return (
    <g transform={`translate(${x} ${y})`}>
      {[...paths].map(([color, d]) => (
        <path
          key={color}
          d={d}
          style={{ fill: color }}
          opacity={dim?.(color) ? 0.2 : 1}
          shapeRendering="crispEdges"
        />
      ))}
    </g>
  );
}

/** A line with a soft glow under it, as the native `glowing_line`. */
function GlowLine({
  d,
  color,
  width,
  opacity = 1,
}: {
  d: string;
  color: string;
  width: number;
  opacity?: number;
}) {
  const common = {
    d,
    fill: "none",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    style: { stroke: color },
  };
  return (
    <g opacity={opacity} className="visual-glow-line">
      <path {...common} strokeOpacity={0.07} strokeWidth={width + 10} />
      <path {...common} strokeOpacity={0.14} strokeWidth={width + 5} />
      <path {...common} strokeWidth={width} />
    </g>
  );
}

/** A square marker in a faint halo of its colour. */
function Marker({ x, y, color }: { x: number; y: number; color: string }) {
  const side = 2 * CELL;
  return (
    <g style={{ fill: color }}>
      <rect
        x={x - (side + 6) / 2}
        y={y - (side + 6) / 2}
        width={side + 6}
        height={side + 6}
        opacity={0.25}
      />
      <rect x={x - side / 2} y={y - side / 2} width={side} height={side} />
    </g>
  );
}

const BAR_LEVEL = 0.75;
const BAR_FLOOR = 0.125;
const BAR_FADE = 0.7;
const MAX_BAR = 36;
const AREA_LEVEL = 0.625;
// Share of the plot each density band reaches below the line, and its
// coverage: densest just under the line, thinning toward the axis.
const AREA_BANDS: [number, number][] = [
  [0.05, 1],
  [0.13, 0.8],
  [0.25, 0.65],
  [0.4, 0.5],
  [0.6, 0.35],
];
const AREA_REST = 0.2;

type Plot = {
  left: number;
  top: number;
  plotW: number;
  plotH: number;
  count: number;
  scale: Scale;
  xOf: (index: number) => number;
  yOf: (value: number) => number;
  band: number;
  tints: string[];
  marks: ReactNode;
  axis: ReactNode;
};

function plot(
  kind: "bar" | "line" | "area",
  series: Series[],
  labels: string[],
  width: number,
  height: number,
  dim: (color: string) => boolean,
): Plot | null {
  const bars = kind === "bar";
  const count = Math.max(
    labels.length,
    ...series.map((s) => s.values.length),
    0,
  );
  if (count === 0 || width <= 0) return null;
  const scale = Scale.nice(
    series.flatMap((s) => s.values),
    kind !== "line",
  );
  const ticks = scale.ticks();
  const tickLabels = ticks.map((t) => scale.label(t));
  const longest = Math.max(1, ...tickLabels.map((l) => l.length));
  const left = Math.ceil(longest * 6.1) + 10;
  const top = bars ? 16 : 8;
  const right = width - (bars ? 0 : 6);
  const cols = Math.max(0, Math.floor((right - left) / CELL));
  const rows = Math.max(0, Math.floor((height - top - 22) / CELL));
  const plotW = cols * CELL;
  const plotH = rows * CELL;
  const yOf = (v: number) => scale.depth(v) * plotH;
  const band = plotW / count;
  const xOf = (i: number) =>
    bars
      ? band * (i + 0.5)
      : count <= 1
        ? plotW / 2
        : (i * plotW) / (count - 1);
  const tints = series.map((s, i) =>
    visualColor(s.color ?? (`series${(i % 6) + 1}` as ThemeColor)),
  );
  const stride = Math.max(
    1,
    Math.ceil(count / Math.max(1, Math.floor(plotW / 72))),
  );

  let marks: ReactNode;
  if (bars) {
    const n = Math.max(1, series.length);
    const widthPx = Math.min((band * 0.7 - CELL * (n - 1)) / n, MAX_BAR);
    const wide = Math.max(1, Math.round(widthPx / CELL));
    const group = wide * n + (n - 1);
    const rowOf = (v: number) => Math.round((scale.depth(v) * plotH) / CELL);
    const zero = rowOf(Math.min(Math.max(scale.min, 0), scale.max));
    const fade = Math.max(1, rows * BAR_FADE);
    const columns: ([number, number, number] | null)[] = new Array(cols).fill(
      null,
    );
    let lastTop: number | null = null;
    for (let i = 0; i < count; i++) {
      const start = Math.round((band * (i + 0.5)) / CELL - group / 2);
      series.forEach((s, j) => {
        const value = s.values[i];
        if (value === undefined) return;
        const end = rowOf(value);
        const first = start + j * (wide + 1);
        for (let col = first; col < first + wide; col++)
          if (col >= 0 && col < cols)
            columns[col] = [Math.min(end, zero), Math.max(end, zero), j];
        if (i === count - 1) lastTop = Math.min(end, zero);
      });
    }
    const latest = series[0]?.values[count - 1];
    marks = (
      <>
        <Cells
          x={left}
          y={top}
          dim={dim}
          paths={cells(cols, rows, (col, row) => {
            const bar = columns[col];
            if (!bar || row < bar[0] || row >= bar[1]) return null;
            const tone =
              BAR_LEVEL -
              (BAR_LEVEL - BAR_FLOOR) * Math.min((row - bar[0]) / fade, 1);
            return row === bar[0] || inked(col, row, tone)
              ? tints[bar[2]]
              : null;
          })}
        />
        {/* With one series, the latest value is written over its bar. */}
        {series.length === 1 && lastTop !== null && latest !== undefined ? (
          <text
            {...AXIS_STYLE}
            fill={mix("var(--color-content)", 90)}
            x={left + band * (count - 0.5)}
            y={top + (lastTop as number) * CELL - 5}
            textAnchor="middle"
          >
            {compact(latest, scale.step / 10)}
          </text>
        ) : null}
      </>
    );
  } else {
    const curves = series.map(
      (s) =>
        new Curve(
          s.values.map((v, i) => [xOf(i), yOf(v)] as [number, number]),
        ),
    );
    const baseline = yOf(Math.max(scale.min, 0));
    marks = (
      <g transform={`translate(${left} ${top})`}>
        {kind === "area" ? (
          <>
            {curves.map((curve, index) => (
              <path
                key={`wash${index}`}
                d={`${curve.path(4)}L${curve.lastX} ${baseline}L${curve.firstX} ${baseline}z`}
                style={{ fill: tints[index] }}
                fillOpacity={0.06}
                opacity={dim(tints[index]) ? 0.2 : 1}
              />
            ))}
            <Cells
              dim={dim}
              paths={cells(cols, rows, (col, row) => {
                const [x, y] = center(col, row);
                if (y > baseline) return null;
                // Later series sit in front, so overlapping fills never mix.
                for (let index = curves.length - 1; index >= 0; index--) {
                  const curve = curves[index];
                  if (x < curve.firstX || x > curve.lastX || y < curve.at(x))
                    continue;
                  const depth = (y - curve.at(x)) / Math.max(plotH, 1);
                  const lead = Math.max(
                    (2 * CELL) / Math.max(plotH, 1),
                    AREA_BANDS[0][0],
                  );
                  const share =
                    depth < lead
                      ? 1
                      : (AREA_BANDS.find(([d]) => depth < d)?.[1] ??
                        AREA_REST);
                  return inked(col, row, step(AREA_LEVEL * share))
                    ? tints[index]
                    : null;
                }
                return null;
              })}
            />
          </>
        ) : null}
        {curves.map((curve, index) => (
          <GlowLine
            key={`line${index}`}
            d={curve.path(2)}
            color={tints[index]}
            width={2}
            opacity={dim(tints[index]) ? 0.25 : 1}
          />
        ))}
      </g>
    );
  }

  const axis = (
    <>
      {ticks.map((tick, index) => {
        const y = top + Math.round(yOf(tick));
        return (
          <g key={tick}>
            <rect
              x={left}
              y={y}
              width={plotW}
              height={1}
              style={{ fill: mix("var(--color-content)", 7) }}
            />
            <text {...AXIS_STYLE} x={left - 10} y={y + 3.5} textAnchor="end">
              {tickLabels[index]}
            </text>
          </g>
        );
      })}
      {labels.map((label, i) =>
        i % stride ? null : (
          <text
            key={i}
            {...AXIS_STYLE}
            x={left + xOf(i)}
            y={top + plotH + 16}
            textAnchor={
              bars
                ? "middle"
                : i === 0
                  ? "start"
                  : i + 1 === count
                    ? "end"
                    : "middle"
            }
          >
            {label.length > 12 ? `${label.slice(0, 11)}…` : label}
          </text>
        ),
      )}
    </>
  );
  return {
    left,
    top,
    plotW,
    plotH,
    count,
    scale,
    xOf,
    yOf,
    band,
    tints,
    marks,
    axis,
  };
}

/** Bars, lines or areas over a ticked axis, with a readout on hover. */
export function Cartesian({
  kind,
  series,
  labels,
  height,
}: {
  kind: "bar" | "line" | "area";
  series: Series[];
  labels: string[];
  height: number;
}) {
  const [box, width] = useWidth<HTMLDivElement>();
  const [active, setActive] = useState<number | null>(null);
  const [spot, setSpot] = useState<{ index: number; locked: boolean } | null>(
    null,
  );
  const spotTint =
    spot != null
      ? visualColor(
          series[spot.index]?.color ??
            (`series${(spot.index % 6) + 1}` as ThemeColor),
        )
      : null;
  const geometry = useMemo(
    () =>
      plot(
        kind,
        series,
        labels,
        width,
        height,
        (color) => spotTint != null && color !== spotTint,
      ),
    [kind, series, labels, width, height, spotTint],
  );
  const label = (index: number) => labels[index] ?? `#${index + 1}`;
  const format = (value: number | undefined) =>
    value === undefined || !geometry
      ? "–"
      : compact(value, geometry.scale.step / 10);
  const summary = `${kind} chart of ${series.map((s) => s.label).join(", ")}, ${
    geometry?.count ?? 0
  } points`;

  const pick = (event: PointerEvent<SVGSVGElement>) => {
    if (!geometry) return;
    const x =
      event.clientX -
      event.currentTarget.getBoundingClientRect().left -
      geometry.left;
    let best = 0;
    for (let i = 1; i < geometry.count; i++)
      if (
        Math.abs(geometry.xOf(i) - x) < Math.abs(geometry.xOf(best) - x)
      )
        best = i;
    setActive(best);
  };
  const keys = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!geometry) return;
    const last = geometry.count - 1;
    const page = Math.max(1, Math.round(geometry.count / 10));
    const moves: Record<string, (current: number) => number> = {
      ArrowLeft: (i) => i - 1,
      ArrowRight: (i) => i + 1,
      PageUp: (i) => i - page,
      PageDown: (i) => i + page,
      Home: () => 0,
      End: () => last,
    };
    if (event.key === "Escape") {
      setActive(null);
      setSpot(null);
      return;
    }
    const move = moves[event.key];
    if (!move) return;
    event.preventDefault();
    setActive((current) =>
      Math.min(Math.max(move(current ?? (event.key === "ArrowLeft" ? last + 1 : -1)), 0), last),
    );
  };

  const readout =
    geometry && active != null && active < geometry.count ? (
      <div
        className="pointer-events-none absolute z-10 flex min-w-28 flex-col gap-1 rounded-md border border-content/10 bg-background-base px-2.5 py-2 text-[11px] shadow-lg"
        style={{
          left: Math.min(
            Math.max(geometry.left + geometry.xOf(active) + 12, 0),
            Math.max(width - 140, 0),
          ),
          top: geometry.top,
        }}
      >
        <span className="text-content/55">{label(active)}</span>
        {series.map((s, index) => (
          <span key={index} className="flex items-center gap-2">
            <span
              className="size-2 shrink-0"
              style={{ background: geometry.tints[index] }}
            />
            <span className="min-w-0 flex-1 truncate text-content/75">
              {s.label}
            </span>
            <span className="font-mono text-content">
              {format(s.values[active])}
            </span>
          </span>
        ))}
      </div>
    ) : null;

  return (
    <div className="flex min-w-0 flex-col gap-2">
      <div
        ref={box}
        role="group"
        aria-label={summary}
        tabIndex={0}
        onKeyDown={keys}
        onBlur={() => setActive(null)}
        className="relative min-w-0 rounded-sm outline-none focus-visible:outline-1 focus-visible:outline-accent"
        style={{ height }}
      >
        {geometry ? (
          <svg
            width={width}
            height={height}
            className="block"
            aria-hidden="true"
            onPointerMove={pick}
            onPointerLeave={() => setActive(null)}
          >
            {geometry.axis}
            {active != null && kind === "bar" ? (
              <rect
                x={geometry.left + geometry.band * active}
                y={geometry.top}
                width={geometry.band}
                height={geometry.plotH}
                style={{ fill: mix("var(--color-content)", 5) }}
              />
            ) : null}
            {geometry.marks}
            {active != null && kind !== "bar" ? (
              <g>
                <rect
                  x={geometry.left + geometry.xOf(active)}
                  y={geometry.top}
                  width={1}
                  height={geometry.plotH}
                  style={{ fill: mix("var(--color-content)", 30) }}
                />
                {series.map((s, index) => {
                  const value = s.values[active];
                  if (value === undefined) return null;
                  return (
                    <Marker
                      key={index}
                      x={geometry.left + geometry.xOf(active)}
                      y={geometry.top + geometry.yOf(value)}
                      color={geometry.tints[index]}
                    />
                  );
                })}
              </g>
            ) : kind !== "bar" ? (
              series.map((s, index) => {
                const last = s.values.length - 1;
                if (last < 0) return null;
                return (
                  <Marker
                    key={index}
                    x={geometry.left + geometry.xOf(last)}
                    y={geometry.top + geometry.yOf(s.values[last])}
                    color={geometry.tints[index]}
                  />
                );
              })
            ) : null}
          </svg>
        ) : null}
        {readout}
        <span className="sr-only" aria-live="polite">
          {active != null
            ? `${label(active)}: ${series
                .map((s) => `${s.label} ${format(s.values[active])}`)
                .join(", ")}`
            : ""}
        </span>
      </div>
      {series.length > 1 && geometry ? (
        <div className="flex flex-wrap gap-x-3 gap-y-1">
          {series.map((s, index) => (
            <button
              key={index}
              type="button"
              aria-pressed={spot?.index === index && spot.locked}
              onPointerEnter={() =>
                setSpot((current) =>
                  current?.locked ? current : { index, locked: false },
                )
              }
              onPointerLeave={() =>
                setSpot((current) => (current?.locked ? current : null))
              }
              onClick={() =>
                setSpot((current) =>
                  current?.locked && current.index === index
                    ? null
                    : { index, locked: true },
                )
              }
              className={`flex items-center gap-1.5 rounded-md px-1 text-[11px] hover:bg-content/8 ${
                spot && spot.index !== index
                  ? "text-content/35"
                  : "text-content/70"
              }`}
            >
              <span
                className="size-2"
                style={{ background: geometry.tints[index] }}
              />
              {s.label}
            </button>
          ))}
        </div>
      ) : null}
      <table className="sr-only">
        <caption>{summary}</caption>
        <thead>
          <tr>
            <th scope="col">Label</th>
            {series.map((s, index) => (
              <th key={index} scope="col">
                {s.label}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {Array.from({ length: geometry?.count ?? 0 }, (_, row) => (
            <tr key={row}>
              <th scope="row">{label(row)}</th>
              {series.map((s, index) => (
                <td key={index}>{format(s.values[row])}</td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

/** The sparkline under a stat: a line over bands that step lighter downward. */
export function Trend({ values, tint }: { values: number[]; tint: string }) {
  const HEIGHT = 56;
  const LEVELS = [10 / 16, 6 / 16, 3 / 16, 1 / 16];
  const [box, width] = useWidth<HTMLDivElement>();
  const drawn = useMemo(() => {
    if (values.length < 2 || width <= 0) return null;
    const cols = Math.floor(width / CELL);
    const rows = Math.floor(HEIGHT / CELL);
    const w = cols * CELL;
    const h = rows * CELL;
    const low = Math.min(...values);
    const high = Math.max(...values);
    const range = high > low ? high - low : 1;
    const top = 6;
    const floor = h * 0.3;
    const curve = new Curve(
      values.map((v, i) => [
        (i / (values.length - 1)) * w,
        top + (1 - (v - low) / range) * (h - top - floor),
      ]),
    );
    const paths = cells(cols, rows, (col, row) => {
      const [x, y] = center(col, row);
      const depth = y - curve.at(x);
      if (depth < 0) return null;
      return inked(
        col,
        row,
        LEVELS[Math.min(Math.floor(depth / 7), LEVELS.length - 1)],
      )
        ? tint
        : null;
    });
    return { paths, curve, w };
  }, [values, width, tint]);
  return (
    <div ref={box} style={{ height: HEIGHT }} aria-hidden="true">
      {drawn ? (
        <svg width={width} height={HEIGHT} className="block">
          <Cells paths={drawn.paths} />
          <GlowLine d={drawn.curve.path(2)} color={tint} width={1.5} />
          <Marker
            x={drawn.w - CELL}
            y={drawn.curve.at(drawn.w)}
            color={tint}
          />
        </svg>
      ) : null}
    </div>
  );
}

/** A radial dial of dithered blocks. */
export function Gauge({
  label,
  value,
  min,
  max,
  unit,
  tint,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string | null;
  tint: string;
}) {
  const SEGMENTS = 36;
  const SPAN = 270;
  const START = -135;
  const size = 220;
  const [box, width] = useWidth<HTMLDivElement>();
  const ratio = Math.min(Math.max((value - min) / (max - min || 1), 0), 1);
  const format = (v: number) =>
    unit === "%"
      ? `${compact(v, 1)}%`
      : unit
        ? `${compact(v, 0.1)} ${unit}`
        : compact(v, 0.1);
  const outerFull = size / 2 - 1 - CELL;
  const height = size / 2 + (outerFull + CELL) * Math.SQRT1_2 + 18;
  const drawn = useMemo(() => {
    if (width <= 0) return null;
    const side = Math.min(size, width);
    const offset = Math.round((width - side) / 2);
    const c = side / 2;
    const outer = side / 2 - 1 - CELL;
    const thickness = Math.max(2, Math.round((side * 0.12) / CELL)) * CELL;
    const inner = outer - thickness;
    const segment = SPAN / SEGMENTS;
    const middle = outer - thickness / 2;
    const gap = ((2 / middle) * 180) / Math.PI;
    const filled = ratio * SEGMENTS;
    const active = Math.max(0, Math.ceil(filled - 1e-6));
    const track = mix(tint, 14);
    const paths = cells(
      Math.floor(side / CELL),
      Math.floor(height / CELL),
      (col, row) => {
        const [x, y] = center(col, row);
        const dx = x - c;
        const dy = y - c;
        const r = Math.hypot(dx, dy);
        if (r < inner || r > outer + CELL) return null;
        const along = (Math.atan2(dx, -dy) * 180) / Math.PI - START;
        if (along < 0 || along >= SPAN) return null;
        const index = Math.floor(along / segment);
        const within = along - index * segment;
        if (within < gap / 2 || within > segment - gap / 2) return null;
        const head = active > 0 && index === active - 1;
        if (r > outer && !head) return null;
        if (index >= active) return track;
        if (head) {
          const part = Math.min(Math.max(filled - index, 0), 1);
          if (within > gap / 2 + (segment - gap) * Math.max(part, 0.15))
            return r <= outer ? track : null;
          return tint;
        }
        const tone = Math.max(1 - (active - 1 - index) / 16, 9 / 16);
        return inked(col, row, tone) ? tint : null;
      },
    );
    return { paths, side, offset, c, thickness, ends: middle * Math.SQRT1_2 };
  }, [width, ratio, tint, height]);
  return (
    <div
      ref={box}
      style={{ height }}
      role="img"
      aria-label={`${label}: ${format(value)} of ${compact(min, 1)} to ${compact(max, 1)}`}
    >
      {drawn ? (
        <svg width={width} height={height} className="block">
          <Cells paths={drawn.paths} x={drawn.offset} />
          <text
            x={drawn.offset + drawn.c}
            y={drawn.c - 4 + drawn.side * 0.05}
            fontSize={Math.round(drawn.side * 0.15)}
            fontWeight={600}
            style={{ fill: "var(--color-content)" }}
            textAnchor="middle"
          >
            {format(value)}
          </text>
          <text
            x={drawn.offset + drawn.c}
            y={drawn.c + drawn.side * 0.1 + 10}
            fontSize={12}
            style={{ fill: mix("var(--color-content)", 60) }}
            textAnchor="middle"
          >
            {label}
          </text>
          {(
            [
              [-drawn.ends, min],
              [drawn.ends, max],
            ] as const
          ).map(([dx, v]) => (
            <text
              key={dx}
              {...AXIS_STYLE}
              x={drawn.offset + drawn.c + dx}
              y={drawn.c + drawn.ends + drawn.thickness / 2 + 15}
              textAnchor="middle"
            >
              {compact(v, 1)}
            </text>
          ))}
        </svg>
      ) : null}
    </div>
  );
}

/** Parts of a whole as a dithered ring with a legend. */
export function Donut({
  slices,
  label,
  value,
}: {
  slices: Slice[];
  label?: string | null;
  value?: string | null;
}) {
  const SIZE = 184;
  const FLOOR = 3 / 16;
  const total = slices.reduce((sum, s) => sum + Math.max(s.value, 0), 0);
  const figure =
    value ??
    (Math.abs(total) >= 1e4
      ? Math.round(total).toLocaleString()
      : compact(total, 0.1));
  const tints = slices.map((s, i) =>
    visualColor(s.color ?? (`series${(i % 6) + 1}` as ThemeColor)),
  );
  const grid = Math.floor(SIZE / CELL);
  const half = (grid * CELL) / 2;
  const outer = half - 2 * CELL;
  const inner = outer - Math.round((SIZE * 0.17) / CELL) * CELL;
  const [hover, setHover] = useState<number | null>(null);
  const paths = useMemo(() => {
    const TAU = Math.PI * 2;
    const angleOf = (dx: number, dy: number) => {
      const a = Math.atan2(dx, -dy);
      return a < 0 ? a + TAU : a;
    };
    const edges: number[] = [];
    let running = 0;
    for (const s of slices) {
      edges.push((running / Math.max(total, 1e-9)) * TAU);
      running += Math.max(s.value, 0);
    }
    const shown = slices.filter((s) => s.value > 0).length;
    return cells(grid, grid, (col, row) => {
      const [x, y] = center(col, row);
      const dx = x - half;
      const dy = y - half;
      const r = Math.hypot(dx, dy);
      if (r < inner || r > outer) return null;
      const angle = angleOf(dx, dy);
      let index = -1;
      for (let i = edges.length - 1; i >= 0; i--)
        if (angle >= edges[i]) {
          index = i;
          break;
        }
      if (index < 0) return null;
      let outline = r > outer - CELL || r < inner + CELL;
      if (shown > 1) {
        let near = Infinity;
        edges.forEach((edge, i) => {
          if (slices[i].value <= 0) return;
          let d = Math.abs(angle - edge);
          d = Math.min(d, TAU - d);
          if (d <= Math.PI / 2) near = Math.min(near, (r * Math.sin(d)) / CELL);
        });
        if (near < 0.75) return null;
        outline ||= near < 1.75;
      }
      const depth = (outer - r) / (outer - inner);
      const tone = FLOOR + (1 - FLOOR) * (1 - depth) ** 1.4;
      return outline || inked(col, row, tone) ? tints[index] : null;
    });
  }, [slices, tints.join(), total, grid, half, inner, outer]);
  return (
    <div className="flex flex-wrap items-center gap-6">
      <svg width={SIZE} height={SIZE} className="block shrink-0" aria-hidden="true">
        <Cells
          paths={paths}
          dim={(color) => hover != null && color !== tints[hover]}
        />
        {label ? (
          <text {...AXIS_STYLE} x={half} y={half - 12} textAnchor="middle">
            {label.toUpperCase().slice(0, 18)}
          </text>
        ) : null}
        <text
          x={half}
          y={half + (label ? 12 : 8)}
          fontSize={Math.floor(
            Math.min(
              26,
              (inner * 2 - 16) / (Math.max(figure.length, 1) * 0.62),
            ),
          )}
          fontWeight={600}
          style={{ fill: "var(--color-content)" }}
          textAnchor="middle"
        >
          {figure}
        </text>
      </svg>
      <ul className="flex min-w-36 flex-1 flex-col gap-2">
        {slices.map((s, index) => (
          <li
            key={`${s.label}${index}`}
            onPointerEnter={() => setHover(index)}
            onPointerLeave={() => setHover(null)}
            className="flex items-center gap-2 text-[12px]"
          >
            <span
              className="size-2.5 shrink-0"
              style={{ background: tints[index] }}
            />
            <span className="min-w-0 flex-1 truncate text-content/80">
              {s.label}
            </span>
            <span className="font-mono text-content/50">
              {compact(s.value, 0.1)} ·{" "}
              {(total > 0 ? (s.value / total) * 100 : 0).toFixed(0)}%
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}

/** Values by row and column as dithered squares. */
export function Heatmap({
  values,
  rows,
  columns,
  tint,
}: {
  values: number[][];
  rows: string[];
  columns: string[];
  tint: string;
}) {
  const SQUARE = 4;
  const PITCH = 5;
  const span = values[0]?.length ?? 0;
  const gutter = rows.length ? 34 : 0;
  const top = columns.length ? 18 : 0;
  const height = top + values.length * PITCH * CELL;
  const [box, available] = useWidth<HTMLDivElement>();
  const drawn = useMemo(() => {
    if (available <= 0) return null;
    const high = Math.max(1e-9, ...values.flat());
    const track = mix("var(--color-content)", 7);
    const cols = Math.floor((available - gutter) / CELL);
    // A narrow chat keeps the latest columns, as calendars do.
    const fits = Math.max(1, Math.floor((cols + PITCH - SQUARE) / PITCH));
    const skip = Math.max(0, span - fits);
    const paths = cells(cols, values.length * PITCH, (col, row) => {
      const x = col % PITCH;
      const y = row % PITCH;
      if (x >= SQUARE || y >= SQUARE || Math.floor(col / PITCH) >= fits)
        return null;
      const v = values[Math.floor(row / PITCH)]?.[Math.floor(col / PITCH) + skip];
      if (v === undefined) return null;
      if (v <= 0) return track;
      return inked(x, y, step(0.25 + 0.75 * (v / high))) ? tint : track;
    });
    return { paths, skip };
  }, [values, available, gutter, span, tint]);
  const pitch = PITCH * CELL;
  let previous: [string, number] | null = null;
  return (
    <div ref={box} style={{ height }} role="img" aria-label="Heatmap">
      {drawn ? (
        <svg width={available} height={height} className="block">
          <Cells paths={drawn.paths} x={gutter} y={top} />
          {rows.map((label, i) =>
            rows.length > 4 && i % 2 === 1 ? null : (
              <text
                key={i}
                {...AXIS_STYLE}
                x={0}
                y={top + i * pitch + (SQUARE * CELL) / 2 + 3.5}
              >
                {label}
              </text>
            ),
          )}
          {columns.slice(drawn.skip).map((label, i) => {
            const x = i * pitch;
            if (previous && (previous[0] === label || x - previous[1] < 36))
              return null;
            previous = [label, x];
            return (
              <text key={i} {...AXIS_STYLE} x={gutter + x} y={top / 2 + 3.5}>
                {label}
              </text>
            );
          })}
        </svg>
      ) : null}
    </div>
  );
}

/** A labelled bar of dithered cells, denser toward its head. */
export function Progress({
  label,
  value,
  max,
  detail,
  tint,
}: {
  label: string;
  value: number;
  max: number;
  detail?: string | null;
  tint: string;
}) {
  const ratio = Math.min(Math.max(value / (max || 1), 0), 1);
  const [box, width] = useWidth<HTMLDivElement>();
  const paths = useMemo(() => {
    const cols = Math.floor(width / CELL);
    const head = Math.round(cols * ratio);
    const track = mix(tint, 14);
    return cells(cols, 3, (col, row) => {
      if (col >= head) return track;
      const tone = 0.3 + 0.7 * (col / Math.max(head, 1)) ** 1.5;
      return col >= head - 3 || inked(col, row, tone) ? tint : track;
    });
  }, [width, ratio, tint]);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-baseline gap-2">
        <span className="min-w-0 flex-1 truncate text-[13px] text-content">
          {label}
        </span>
        <span className="font-mono text-[12px] text-content/55">
          {(ratio * 100).toFixed(0)}%
        </span>
      </div>
      <div
        ref={box}
        style={{ height: 3 * CELL }}
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={max}
        aria-valuenow={value}
      >
        {width > 0 ? (
          <svg width={width} height={3 * CELL} className="block">
            <Cells paths={paths} />
          </svg>
        ) : null}
      </div>
      {detail ? (
        <span className="text-[12px] text-content/55">{detail}</span>
      ) : null}
    </div>
  );
}
