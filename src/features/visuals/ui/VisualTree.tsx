// A visual's component tree as the chat draws it. The program describes
// content; the app owns fonts, colours, spacing and controls, as the native
// renderer does (crates/monocode-gpui/src/visuals.rs).
import { useEffect, useRef, useState } from "react";

import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";
import { Switch } from "../../../shared/ui/Switch";
import type { Shape, TextSize, VisualNode } from "../model/visual";
import {
  Cartesian,
  Donut,
  Gauge,
  Heatmap,
  mix,
  Progress,
  Trend,
  useWidth,
  visualColor,
} from "./charts";
import { Scene } from "./Scene";

/** Sends a control's new value to the program; it answers with a new tree. */
export type VisualDispatch = (handler: string, value: unknown) => void;

const TEXT_SIZE: Record<TextSize, string> = {
  caption: "text-[11px]",
  small: "text-[12px]",
  body: "text-[13px] leading-relaxed",
  subtitle: "text-[15px] font-medium",
  title: "text-[18px] font-semibold tracking-tight",
  hero: "text-[30px] font-semibold tracking-tight",
};

/** Whether a signed change such as "-3.2%" falls. */
const falls = (delta: string) => /^\s*[-−↓]/.test(delta);

export function VisualTree({
  node,
  dispatch,
  disabled = false,
}: {
  node: VisualNode;
  dispatch: VisualDispatch;
  /** While a change is on its way, controls wait for its answer. */
  disabled?: boolean;
}) {
  const child = (entry: VisualNode, index: number) => (
    <VisualTree
      key={index}
      node={entry}
      dispatch={dispatch}
      disabled={disabled}
    />
  );
  switch (node.kind) {
    case "column":
      return (
        <div className="flex min-w-0 flex-col" style={{ gap: node.gap }}>
          {node.children.map(child)}
        </div>
      );
    case "row":
      return (
        <div
          className="flex min-w-0 flex-wrap items-start [&>*]:min-w-0 [&>*]:flex-1"
          style={{ gap: node.gap }}
        >
          {node.children.map(child)}
        </div>
      );
    case "grid":
      return (
        <div
          className="grid min-w-0"
          style={{
            gap: node.gap,
            gridTemplateColumns: `repeat(auto-fit, minmax(min(100%, max(160px, calc((100% - ${
              (node.columns - 1) * node.gap
            }px) / ${node.columns}))), 1fr))`,
          }}
        >
          {node.children.map(child)}
        </div>
      );
    case "card":
      return (
        <div
          className="flex min-w-0 flex-col rounded-lg border border-content/10 bg-content/3 p-4"
          style={{ gap: node.gap }}
        >
          {node.children.map(child)}
        </div>
      );
    case "text":
      return (
        <p
          className={`min-w-0 break-words whitespace-pre-wrap ${TEXT_SIZE[node.size]}`}
          style={{ color: visualColor(node.color) }}
        >
          {node.text}
        </p>
      );
    case "stat": {
      const good = node.delta
        ? falls(node.delta) === !!node.lower_is_better
        : undefined;
      return (
        <div className="flex min-w-0 flex-col gap-1">
          <span className="text-[12px] text-content/60">{node.label}</span>
          <span
            className="text-[24px] font-semibold tracking-tight"
            style={{ color: visualColor(node.color) }}
          >
            {node.value}
          </span>
          {node.delta || node.detail ? (
            <span className="flex flex-wrap items-baseline gap-1.5 text-[12px] text-content/55">
              {node.delta ? (
                <span
                  className="font-mono font-medium"
                  style={{
                    color: visualColor(good ? "positive" : "negative"),
                  }}
                >
                  {falls(node.delta) ? "↓" : "↑"}{" "}
                  {node.delta.replace(/^\s*[↑↓]\s*/, "")}
                </span>
              ) : null}
              {node.detail}
            </span>
          ) : null}
          {node.trend && node.trend.length > 1 ? (
            <Trend
              values={node.trend}
              tint={visualColor(node.color === "ink" ? "accent" : node.color)}
            />
          ) : null}
        </div>
      );
    }
    case "table":
      return (
        <div className="min-w-0 overflow-x-auto">
          <table className="w-full border-collapse text-[12px]">
            <thead>
              <tr>
                {node.columns.map((column, index) => (
                  <th
                    key={index}
                    scope="col"
                    className="border-b border-content/10 px-2 py-1.5 text-left font-medium text-content/60"
                  >
                    {column}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {node.rows.map((row, rowIndex) => (
                <tr key={rowIndex} className="hover:bg-content/4">
                  {row.map((cell, index) => (
                    <td
                      key={index}
                      className="border-b border-content/6 px-2 py-1.5 text-content/85"
                    >
                      {cell}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );
    case "chart":
      return (
        <Cartesian
          kind={node.chart_type}
          series={node.series}
          labels={node.labels}
          height={node.height}
        />
      );
    case "gauge":
      return (
        <Gauge
          label={node.label}
          value={node.value}
          min={node.min}
          max={node.max}
          unit={node.unit}
          tint={visualColor(node.color)}
        />
      );
    case "donut":
      return (
        <Donut slices={node.slices} label={node.label} value={node.value} />
      );
    case "heatmap":
      return (
        <Heatmap
          values={node.values}
          rows={node.rows}
          columns={node.columns}
          tint={visualColor(node.color)}
        />
      );
    case "progress":
      return (
        <Progress
          label={node.label}
          value={node.value}
          max={node.max}
          detail={node.detail}
          tint={visualColor(node.color)}
        />
      );
    case "button":
      return (
        <div>
          <SecondaryButton
            disabled={node.disabled || disabled}
            onClick={() => dispatch(node.handler, null)}
          >
            {node.label}
          </SecondaryButton>
        </div>
      );
    case "toggle":
      return (
        <label className="flex items-center gap-2.5 text-[13px] text-content/80">
          <Switch
            label={node.label}
            on={node.value}
            disabled={disabled}
            onChange={(on) => dispatch(node.handler, on)}
          />
          {node.label}
        </label>
      );
    case "input":
      return <VisualInput node={node} dispatch={dispatch} />;
    case "select":
      return (
        <div className="max-w-72">
          <SearchableSelect
            label={node.label}
            value={String(node.selected)}
            options={node.options.map((option, index) => ({
              value: String(index),
              label: option,
            }))}
            searchable={node.options.length > 8}
            disabled={disabled}
            onChange={(value) => dispatch(node.handler, Number(value))}
          />
        </div>
      );
    case "slider":
      return <VisualSlider node={node} dispatch={dispatch} />;
    case "tabs":
      return (
        <div
          role="tablist"
          className="flex flex-wrap gap-1 border-b border-content/10 pb-1"
        >
          {node.labels.map((label, index) => (
            <button
              key={index}
              type="button"
              role="tab"
              aria-selected={index === node.selected}
              disabled={disabled}
              onClick={() =>
                index === node.selected ? undefined : dispatch(node.handler, index)
              }
              className={`rounded-md px-2.5 py-1 text-[12px] ${
                index === node.selected
                  ? "bg-selection text-content"
                  : "text-content/60 hover:bg-content/8 hover:text-content"
              }`}
            >
              {label}
            </button>
          ))}
        </div>
      );
    case "canvas":
      return <VisualCanvas node={node} />;
    case "scene":
      return <Scene node={node} />;
  }
}

function VisualInput({
  node,
  dispatch,
}: {
  node: Extract<VisualNode, { kind: "input" }>;
  dispatch: VisualDispatch;
}) {
  // The field keeps what is typed; the program hears each change.
  const [value, setValue] = useState(node.value);
  const focused = useRef(false);
  useEffect(() => {
    if (!focused.current) setValue(node.value);
  }, [node.value]);
  return (
    <input
      aria-label={node.label}
      value={value}
      placeholder={node.placeholder ?? undefined}
      maxLength={4096}
      onFocus={() => (focused.current = true)}
      onBlur={() => {
        focused.current = false;
        setValue(node.value);
      }}
      onChange={(event) => {
        setValue(event.target.value);
        dispatch(node.handler, event.target.value);
      }}
      className="h-9 w-full max-w-96 rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none focus:border-content/25"
    />
  );
}

function VisualSlider({
  node,
  dispatch,
}: {
  node: Extract<VisualNode, { kind: "slider" }>;
  dispatch: VisualDispatch;
}) {
  // Dragging moves the thumb at once; the program catches up behind it.
  const [value, setValue] = useState(node.value);
  const dragging = useRef(false);
  useEffect(() => {
    if (!dragging.current) setValue(node.value);
  }, [node.value]);
  const decimals = (String(node.step).split(".")[1] ?? "").length;
  return (
    <label className="flex min-w-0 flex-col gap-1.5">
      <span className="flex items-baseline gap-2 text-[12px] text-content/70">
        <span className="min-w-0 flex-1 truncate">{node.label}</span>
        <span className="font-mono text-content">
          {value.toFixed(Math.min(decimals, 6))}
        </span>
      </span>
      <input
        type="range"
        min={node.min}
        max={node.max}
        step={node.step}
        value={value}
        onPointerDown={() => (dragging.current = true)}
        onPointerUp={() => (dragging.current = false)}
        onChange={(event) => {
          const next = Number(event.target.value);
          setValue(next);
          dispatch(node.handler, next);
        }}
        className="w-full accent-accent"
      />
    </label>
  );
}

function shape(entry: Shape, index: number) {
  const color = visualColor(entry.color);
  switch (entry.kind) {
    case "rect":
      return (
        <rect
          key={index}
          x={entry.x}
          y={entry.y}
          width={entry.width}
          height={entry.height}
          style={{ fill: color }}
        />
      );
    case "circle":
      return (
        <circle
          key={index}
          cx={entry.x}
          cy={entry.y}
          r={entry.radius}
          style={{ fill: color }}
        />
      );
    case "line":
      return (
        <line
          key={index}
          x1={entry.x1}
          y1={entry.y1}
          x2={entry.x2}
          y2={entry.y2}
          strokeWidth={entry.width}
          strokeLinecap="round"
          style={{ stroke: color }}
        />
      );
    case "polyline":
      return (
        <polyline
          key={index}
          points={entry.points.map(([x, y]) => `${x},${y}`).join(" ")}
          strokeWidth={entry.width}
          strokeLinecap="round"
          strokeLinejoin="round"
          fill="none"
          style={{ stroke: color }}
        />
      );
    case "label":
      return (
        <text
          key={index}
          x={entry.x}
          y={entry.y}
          fontSize={12}
          style={{ fill: color }}
        >
          {entry.text}
        </text>
      );
  }
}

/** A drawing in its own coordinates, scaled to the chat's width. */
function VisualCanvas({
  node,
}: {
  node: Extract<VisualNode, { kind: "canvas" }>;
}) {
  const [box, width] = useWidth<HTMLDivElement>();
  const scale = width > 0 ? Math.min(1, width / node.width) : 1;
  return (
    <div ref={box} className="min-w-0">
      <svg
        width={node.width * scale}
        height={node.height * scale}
        viewBox={`0 0 ${node.width} ${node.height}`}
        className="block"
        style={{ background: mix("var(--color-content)", 2) }}
        role="img"
      >
        {node.shapes.map(shape)}
      </svg>
    </div>
  );
}
