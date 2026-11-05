import type { ReactNode } from "react";
import {
  axisLabel,
  niceScale,
  stackedSegments,
  ticks,
  valueRange,
  type Scale,
} from "../model/chartScale";
import type { ChartSeries, SheetChart } from "../model/spreadsheetDrawings";
import { readableOn } from "../model/spreadsheet";

const FONT = 10;
const PAD = 8;

/**
 * A workbook's chart drawn from its own series: columns, bars, lines, areas,
 * scatter, pie and doughnut. Text and axes follow the theme; the series keep
 * the colours the file gave them.
 */
export function SpreadsheetChart({ chart }: { chart: SheetChart }) {
  const { width, height } = chart.box;
  const round = chart.series.some(
    (entry) => entry.kind === "pie" || entry.kind === "doughnut",
  );
  const legendItems: { label: string; color?: string }[] = round
    ? chart.categories.map((label, index) => ({
        label,
        color: chart.series[0]?.pointColors?.[index],
      }))
    : chart.series.map((entry) => ({ label: entry.name, color: entry.color }));
  const showLegend = chart.legend && legendItems.length > (round ? 0 : 1);
  const top = PAD + (chart.title ? 20 : 0);
  const bottom = height - PAD - (showLegend ? 18 : 0);

  let body: ReactNode;
  if (chart.unsupported || !chart.series.length)
    body = (
      <text
        x={width / 2}
        y={(top + bottom) / 2}
        textAnchor="middle"
        className="fill-current opacity-60"
        fontSize={11}
      >
        {chart.unsupported
          ? `${chart.unsupported} chart — open the file to see it`
          : "This chart has no data"}
      </text>
    );
  else if (round) body = <Pie chart={chart} top={top} bottom={bottom} />;
  else if (chart.series.every((entry) => entry.kind === "scatter"))
    body = <Scatter chart={chart} top={top} bottom={bottom} />;
  else body = <Categories chart={chart} top={top} bottom={bottom} />;

  return (
    <div
      className="size-full overflow-hidden rounded-sm border border-content/15 bg-background-base text-content"
      role="img"
      aria-label={chart.title ? `Chart: ${chart.title}` : "Chart"}
    >
      <svg width={width} height={height} className="block">
        {chart.title ? (
          <text
            x={width / 2}
            y={PAD + 12}
            textAnchor="middle"
            fontSize={12}
            fontWeight={600}
            className="fill-current"
          >
            {chart.title}
          </text>
        ) : null}
        {body}
        {showLegend ? (
          <Legend items={legendItems} width={width} y={height - PAD - 6} />
        ) : null}
      </svg>
    </div>
  );
}

function Legend({
  items,
  width,
  y,
}: {
  items: { label: string; color?: string }[];
  width: number;
  y: number;
}) {
  // Each entry takes the room its label needs, centred as a row.
  const sizes = items.map((item) => 14 + Math.min(item.label.length, 24) * 5.6);
  const total = sizes.reduce((sum, size) => sum + size + 10, -10);
  let x = Math.max(PAD, (width - total) / 2);
  return (
    <g fontSize={FONT} className="fill-current">
      {items.map((item, index) => {
        const at = x;
        x += sizes[index] + 10;
        if (at + 14 > width) return null;
        return (
          <g key={index}>
            <rect
              x={at}
              y={y - 7}
              width={9}
              height={9}
              rx={1.5}
              fill={item.color}
            />
            <text x={at + 13} y={y + 1} className="opacity-75">
              {item.label.length > 24
                ? `${item.label.slice(0, 23)}…`
                : item.label}
            </text>
          </g>
        );
      })}
    </g>
  );
}

type Frame = { left: number; right: number; top: number; bottom: number };

/** Gridlines and value labels along one axis. */
function ValueAxis({
  scale,
  frame,
  horizontal,
  title,
}: {
  scale: Scale;
  frame: Frame;
  horizontal: boolean;
  title?: string;
}) {
  const at = (value: number) =>
    horizontal
      ? frame.left +
        ((value - scale.min) / (scale.max - scale.min)) *
          (frame.right - frame.left)
      : frame.bottom -
        ((value - scale.min) / (scale.max - scale.min)) *
          (frame.bottom - frame.top);
  return (
    <g fontSize={FONT} className="fill-current">
      {ticks(scale).map((value) => {
        const position = at(value);
        return horizontal ? (
          <g key={value}>
            <line
              x1={position}
              x2={position}
              y1={frame.top}
              y2={frame.bottom}
              stroke="currentColor"
              strokeOpacity={value === 0 ? 0.35 : 0.1}
            />
            <text
              x={position}
              y={frame.bottom + 12}
              textAnchor="middle"
              className="opacity-60"
            >
              {axisLabel(value)}
            </text>
          </g>
        ) : (
          <g key={value}>
            <line
              x1={frame.left}
              x2={frame.right}
              y1={position}
              y2={position}
              stroke="currentColor"
              strokeOpacity={value === 0 ? 0.35 : 0.1}
            />
            <text
              x={frame.left - 4}
              y={position + 3}
              textAnchor="end"
              className="opacity-60"
            >
              {axisLabel(value)}
            </text>
          </g>
        );
      })}
      {title ? (
        horizontal ? (
          <text
            x={(frame.left + frame.right) / 2}
            y={frame.bottom + 26}
            textAnchor="middle"
            className="opacity-70"
          >
            {title}
          </text>
        ) : (
          <text
            transform={`translate(${PAD + 4} ${(frame.top + frame.bottom) / 2}) rotate(-90)`}
            textAnchor="middle"
            className="opacity-70"
          >
            {title}
          </text>
        )
      ) : null}
    </g>
  );
}

function linePath(points: ([number, number] | null)[]): string {
  let path = "";
  let pen = false;
  for (const point of points) {
    if (!point) {
      pen = false;
      continue;
    }
    path += `${pen ? "L" : "M"}${point[0].toFixed(1)} ${point[1].toFixed(1)}`;
    pen = true;
  }
  return path;
}

/** Columns, bars, lines and areas over shared categories. */
function Categories({
  chart,
  top,
  bottom,
}: {
  chart: SheetChart;
  top: number;
  bottom: number;
}) {
  const { width } = chart.box;
  const count = chart.categories.length;
  const horizontal = chart.series.some((entry) => entry.kind === "bar");
  const blocks = chart.series.filter(
    (entry) => entry.kind === "column" || entry.kind === "bar",
  );
  const others = chart.series.filter(
    (entry) => entry.kind === "line" || entry.kind === "area",
  );
  const stacked = stackedSegments(blocks, chart.stacking, count);
  const flat = stackedSegments(
    others,
    others.some((entry) => entry.kind === "area") ? chart.stacking : undefined,
    count,
  );
  const [low, high] = valueRange(
    [...stacked, ...flat],
    blocks.length > 0 || others.some((entry) => entry.kind === "area"),
  );
  const scale = niceScale(low, high);
  const labelWidth =
    Math.max(...ticks(scale).map((value) => axisLabel(value).length)) * 5.6 + 8;
  const categoryWidth = horizontal
    ? Math.min(
        90,
        Math.max(...chart.categories.map((label) => label.length)) * 5.6 + 8,
      )
    : 0;
  const frame: Frame = {
    left:
      PAD + (chart.yTitle ? 14 : 0) + (horizontal ? categoryWidth : labelWidth),
    right: width - PAD - 4,
    top: top + 4,
    bottom: bottom - 16 - (chart.xTitle ? 14 : 0),
  };
  if (frame.right <= frame.left || frame.bottom <= frame.top || !count)
    return null;
  const span = horizontal ? frame.bottom - frame.top : frame.right - frame.left;
  const band = span / count;
  const value = (amount: number) =>
    horizontal
      ? frame.left +
        ((amount - scale.min) / (scale.max - scale.min)) *
          (frame.right - frame.left)
      : frame.bottom -
        ((amount - scale.min) / (scale.max - scale.min)) *
          (frame.bottom - frame.top);
  const center = (index: number) =>
    (horizontal ? frame.top : frame.left) + band * index + band / 2;
  const zero = value(Math.max(scale.min, Math.min(scale.max, 0)));
  const clusters = chart.stacking ? 1 : Math.max(1, blocks.length);
  const barSize = (band * 0.7) / clusters;
  // Category labels thin out when they would overlap.
  const every = horizontal
    ? Math.ceil(12 / band)
    : Math.ceil(
        (Math.max(
          ...chart.categories.map((label) => Math.min(label.length, 14)),
        ) *
          5.6 +
          6) /
          band,
      );

  return (
    <g>
      <ValueAxis
        scale={scale}
        frame={frame}
        horizontal={horizontal}
        title={horizontal ? chart.xTitle : chart.yTitle}
      />
      {!horizontal && chart.xTitle ? (
        <text
          x={(frame.left + frame.right) / 2}
          y={frame.bottom + 28}
          textAnchor="middle"
          fontSize={FONT}
          className="fill-current opacity-70"
        >
          {chart.xTitle}
        </text>
      ) : null}
      {horizontal && chart.yTitle ? (
        <text
          transform={`translate(${PAD + 4} ${(frame.top + frame.bottom) / 2}) rotate(-90)`}
          textAnchor="middle"
          fontSize={FONT}
          className="fill-current opacity-70"
        >
          {chart.yTitle}
        </text>
      ) : null}
      <g fontSize={FONT} className="fill-current">
        {chart.categories.map((label, index) =>
          index % Math.max(1, every) ? null : (
            <text
              key={index}
              x={horizontal ? frame.left - 4 : center(index)}
              y={horizontal ? center(index) + 3 : frame.bottom + 12}
              textAnchor={horizontal ? "end" : "middle"}
              className="opacity-60"
            >
              {label.length > 14 ? `${label.slice(0, 13)}…` : label}
            </text>
          ),
        )}
      </g>
      {blocks.map((entry, seriesIndex) =>
        stacked[seriesIndex].map((segment, index) => {
          if (entry.values[index] === null || entry.values[index] === undefined)
            return null;
          const offset = chart.stacking ? 0 : seriesIndex;
          const start =
            center(index) - (barSize * clusters) / 2 + offset * barSize;
          const a = chart.stacking ? value(segment.from) : zero;
          const b = value(segment.to);
          return horizontal ? (
            <rect
              key={`${seriesIndex}:${index}`}
              x={Math.min(a, b)}
              y={start}
              width={Math.abs(b - a)}
              height={Math.max(1, barSize - 1)}
              fill={entry.color}
            />
          ) : (
            <rect
              key={`${seriesIndex}:${index}`}
              x={start}
              y={Math.min(a, b)}
              width={Math.max(1, barSize - 1)}
              height={Math.abs(b - a)}
              fill={entry.color}
            />
          );
        }),
      )}
      {others.map((entry, seriesIndex) => (
        <SeriesLine
          key={seriesIndex}
          entry={entry}
          points={flat[seriesIndex].map((segment, index) =>
            entry.values[index] === null || entry.values[index] === undefined
              ? null
              : horizontal
                ? [value(segment.to), center(index)]
                : [center(index), value(segment.to)],
          )}
          baseline={(point) =>
            horizontal ? [zero, point[1]] : [point[0], zero]
          }
        />
      ))}
    </g>
  );
}

function SeriesLine({
  entry,
  points,
  baseline,
}: {
  entry: ChartSeries;
  points: ([number, number] | null)[];
  baseline: (point: [number, number]) => [number, number];
}) {
  const path = linePath(points);
  if (entry.kind === "area") {
    const drawn = points.filter((point): point is [number, number] => !!point);
    if (drawn.length < 2) return null;
    const first = baseline(drawn[0]);
    const last = baseline(drawn[drawn.length - 1]);
    return (
      <path
        d={`${path}L${last[0]} ${last[1]}L${first[0]} ${first[1]}Z`}
        fill={entry.color}
        fillOpacity={0.55}
        stroke={entry.color}
      />
    );
  }
  return (
    <g>
      <path
        d={path}
        fill="none"
        stroke={entry.color}
        strokeWidth={2}
        strokeLinejoin="round"
      />
      {points.length <= 60
        ? points.map((point, index) =>
            point ? (
              <circle
                key={index}
                cx={point[0]}
                cy={point[1]}
                r={2.5}
                fill={entry.color}
              />
            ) : null,
          )
        : null}
    </g>
  );
}

function Scatter({
  chart,
  top,
  bottom,
}: {
  chart: SheetChart;
  top: number;
  bottom: number;
}) {
  const { width } = chart.box;
  const xs = chart.series
    .flatMap((entry) => entry.xValues ?? [])
    .filter((v): v is number => v !== null);
  const ys = chart.series
    .flatMap((entry) => entry.values)
    .filter((v): v is number => v !== null);
  if (!xs.length || !ys.length) return null;
  const xScale = niceScale(Math.min(...xs), Math.max(...xs));
  const yScale = niceScale(Math.min(...ys), Math.max(...ys));
  const labelWidth =
    Math.max(...ticks(yScale).map((value) => axisLabel(value).length)) * 5.6 +
    8;
  const frame: Frame = {
    left: PAD + (chart.yTitle ? 14 : 0) + labelWidth,
    right: width - PAD - 8,
    top: top + 4,
    bottom: bottom - 16 - (chart.xTitle ? 14 : 0),
  };
  if (frame.right <= frame.left || frame.bottom <= frame.top) return null;
  const x = (value: number) =>
    frame.left +
    ((value - xScale.min) / (xScale.max - xScale.min)) *
      (frame.right - frame.left);
  const y = (value: number) =>
    frame.bottom -
    ((value - yScale.min) / (yScale.max - yScale.min)) *
      (frame.bottom - frame.top);
  return (
    <g>
      <ValueAxis
        scale={yScale}
        frame={frame}
        horizontal={false}
        title={chart.yTitle}
      />
      <ValueAxis scale={xScale} frame={frame} horizontal title={chart.xTitle} />
      {chart.series.map((entry, index) => (
        <SeriesLine
          key={index}
          entry={{ ...entry, kind: "line" }}
          points={entry.values.map((value, point) => {
            const at = entry.xValues?.[point];
            return value === null || at === null || at === undefined
              ? null
              : [x(at), y(value)];
          })}
          baseline={(point) => point}
        />
      ))}
    </g>
  );
}

function Pie({
  chart,
  top,
  bottom,
}: {
  chart: SheetChart;
  top: number;
  bottom: number;
}) {
  const series = chart.series[0];
  const values = series.values.map((value) => Math.max(0, value ?? 0));
  const total = values.reduce((sum, value) => sum + value, 0);
  const radius = Math.max(
    0,
    Math.min(chart.box.width - PAD * 2, bottom - top) / 2 - 4,
  );
  if (!total || radius <= 0) return null;
  const cx = chart.box.width / 2;
  const cy = (top + bottom) / 2;
  const inner = series.kind === "doughnut" ? radius * 0.55 : 0;
  let angle = -Math.PI / 2;
  return (
    <g>
      {values.map((value, index) => {
        if (!value) return null;
        const start = angle;
        const sweep = (value / total) * Math.PI * 2;
        angle += sweep;
        const color = series.pointColors?.[index] ?? series.color;
        if (sweep >= Math.PI * 2 - 1e-6)
          return (
            <circle
              key={index}
              cx={cx}
              cy={cy}
              r={(radius + inner) / 2}
              fill="none"
              stroke={color}
              strokeWidth={radius - inner || radius}
            />
          );
        const point = (r: number, a: number) =>
          `${(cx + r * Math.cos(a)).toFixed(2)} ${(cy + r * Math.sin(a)).toFixed(2)}`;
        const large = sweep > Math.PI ? 1 : 0;
        const d = inner
          ? `M${point(radius, start)}A${radius} ${radius} 0 ${large} 1 ${point(radius, angle)}L${point(inner, angle)}A${inner} ${inner} 0 ${large} 0 ${point(inner, start)}Z`
          : `M${cx} ${cy}L${point(radius, start)}A${radius} ${radius} 0 ${large} 1 ${point(radius, angle)}Z`;
        const middle = start + sweep / 2;
        const share = Math.round((value / total) * 100);
        const labelRadius = inner ? (radius + inner) / 2 : radius * 0.65;
        return (
          <g key={index}>
            <path
              d={d}
              fill={color}
              stroke="var(--color-background-base)"
              strokeWidth={1}
            />
            {share >= 5 ? (
              <text
                x={cx + labelRadius * Math.cos(middle)}
                y={cy + labelRadius * Math.sin(middle) + 3}
                textAnchor="middle"
                fontSize={FONT}
                fill={color ? readableOn(color) : undefined}
              >
                {share}%
              </text>
            ) : null}
          </g>
        );
      })}
    </g>
  );
}
