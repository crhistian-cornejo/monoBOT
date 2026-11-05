// A visual an agent published into its chat: a sandboxed program that the
// app runs (src-tauri/src/visuals.rs) and the tree of components it returns
// (crates/monocode-core/src/visual/model.rs). Data, never HTML.
import type { Block, Session } from "../../sessions/model/session";

export type ThemeColor =
  | "ink"
  | "muted"
  | "faint"
  | "accent"
  | "positive"
  | "negative"
  | "warning"
  | "series1"
  | "series2"
  | "series3"
  | "series4"
  | "series5"
  | "series6";
export type TextSize =
  | "caption"
  | "small"
  | "body"
  | "subtitle"
  | "title"
  | "hero";
export type Series = { label: string; values: number[]; color?: ThemeColor };
export type Slice = { label: string; value: number; color?: ThemeColor };

export type Shape =
  | {
      kind: "rect";
      x: number;
      y: number;
      width: number;
      height: number;
      color: ThemeColor;
    }
  | { kind: "circle"; x: number; y: number; radius: number; color: ThemeColor }
  | {
      kind: "line";
      x1: number;
      y1: number;
      x2: number;
      y2: number;
      width: number;
      color: ThemeColor;
    }
  | {
      kind: "polyline";
      points: [number, number][];
      width: number;
      color: ThemeColor;
    }
  | { kind: "label"; x: number; y: number; text: string; color: ThemeColor };

export type SceneObject = {
  geometry:
    | { kind: "box" | "sphere" | "cylinder" | "cone" | "plane" }
    | { kind: "mesh"; vertices: [number, number, number][]; indices: [number, number, number][] }
    | { kind: "line"; points: [number, number, number][]; width: number };
  position: [number, number, number];
  rotation: [number, number, number];
  scale: [number, number, number];
  color: ThemeColor;
};

export type VisualNode =
  | { kind: "column" | "row" | "card"; children: VisualNode[]; gap: number }
  | { kind: "grid"; children: VisualNode[]; columns: number; gap: number }
  | { kind: "text"; text: string; size: TextSize; color: ThemeColor }
  | {
      kind: "stat";
      label: string;
      value: string;
      detail?: string | null;
      color: ThemeColor;
      delta?: string | null;
      trend?: number[];
      lower_is_better?: boolean;
    }
  | { kind: "table"; columns: string[]; rows: string[][] }
  | {
      kind: "chart";
      chart_type: "bar" | "line" | "area";
      series: Series[];
      labels: string[];
      height: number;
    }
  | {
      kind: "gauge";
      label: string;
      value: number;
      min: number;
      max: number;
      unit?: string | null;
      color: ThemeColor;
    }
  | {
      kind: "donut";
      slices: Slice[];
      label?: string | null;
      value?: string | null;
    }
  | {
      kind: "heatmap";
      values: number[][];
      rows: string[];
      columns: string[];
      color: ThemeColor;
    }
  | {
      kind: "progress";
      label: string;
      value: number;
      max: number;
      detail?: string | null;
      color: ThemeColor;
    }
  | { kind: "button"; label: string; handler: string; disabled: boolean }
  | { kind: "toggle"; label: string; value: boolean; handler: string }
  | {
      kind: "input";
      label: string;
      value: string;
      placeholder?: string | null;
      handler: string;
    }
  | {
      kind: "select";
      label: string;
      options: string[];
      selected: number;
      handler: string;
    }
  | {
      kind: "slider";
      label: string;
      value: number;
      min: number;
      max: number;
      step: number;
      handler: string;
    }
  | { kind: "tabs"; labels: string[]; selected: number; handler: string }
  | { kind: "canvas"; width: number; height: number; shapes: Shape[] }
  | {
      kind: "scene";
      width: number;
      height: number;
      objects: SceneObject[];
      camera: {
        eye: [number, number, number];
        target: [number, number, number];
        fov: number;
      };
    };

/** A published visual, kept on the user message of the turn that made it. */
export type VisualDocument = { id: string; title: string; source: string };

const ID = /^[A-Za-z0-9_-]{1,96}$/;
const MAX_SOURCE = 64 * 1024;

/** A document from the agent or from storage, or undefined when malformed. */
export function parseVisualDocument(value: unknown): VisualDocument | undefined {
  if (!value || typeof value !== "object") return undefined;
  const { id, title, source } = value as Record<string, unknown>;
  if (typeof id !== "string" || !ID.test(id)) return undefined;
  if (typeof title !== "string" || !title.trim()) return undefined;
  if (typeof source !== "string" || !source.trim()) return undefined;
  if (new TextEncoder().encode(source).length > MAX_SOURCE) return undefined;
  return { id, title: title.trim().slice(0, 160), source };
}

/** The well-formed visuals of a stored block. */
export function sanitizeVisuals(value: unknown): VisualDocument[] {
  if (!Array.isArray(value)) return [];
  const seen = new Map<string, VisualDocument>();
  for (const entry of value) {
    const document = parseVisualDocument(entry);
    if (document) seen.set(document.id, document);
  }
  return [...seen.values()].slice(-64);
}

/** The user message a visual published now belongs to: the latest one. */
export function visualAnchor(session: Session): string | undefined {
  return session.blocks
    .slice()
    .reverse()
    .find((block) => block.role === "user")?.id;
}

/**
 * Keep `document` on the turn's user message. Publishing the same id again
 * replaces it wherever it is, so a revised chart never shows twice.
 */
export function recordVisual(
  session: Session,
  turnId: string,
  document: VisualDocument,
): Session {
  return {
    ...session,
    blocks: session.blocks.map((block) => {
      const others = (block.visuals ?? []).filter(
        (entry) => entry.id !== document.id,
      );
      if (block.id === turnId) return { ...block, visuals: [...others, document] };
      if (!block.visuals?.some((entry) => entry.id === document.id))
        return block;
      if (others.length) return { ...block, visuals: others };
      const { visuals: _visuals, ...rest } = block;
      return rest;
    }),
  };
}

/** Every visual a conversation holds, by id. */
export function visualsById(
  blocks: readonly Block[],
): ReadonlyMap<string, VisualDocument> {
  const visuals = new Map<string, VisualDocument>();
  for (const block of blocks)
    for (const document of block.visuals ?? []) visuals.set(document.id, document);
  return visuals;
}

export type ReplyPart =
  | { kind: "text"; text: string }
  | { kind: "visual"; id: string; caption: string };

const EMBED = /^\s{0,3}!\[([^\]\n]*)\]\(visual:([A-Za-z0-9_-]{1,96})\)\s*$/;
const FENCE = /^\s{0,3}(`{3,}|~{3,})/;

/**
 * A reply split at each line that is only a visual's embed,
 * `![caption](visual:id)`, outside code. Text between keeps its lines.
 */
export function splitVisualEmbeds(text: string): ReplyPart[] {
  if (!text.includes("](visual:")) return [{ kind: "text", text }];
  const parts: ReplyPart[] = [];
  let buffer: string[] = [];
  let fence: string | null = null;
  const flush = () => {
    const chunk = buffer.join("\n");
    if (chunk.trim()) parts.push({ kind: "text", text: chunk });
    buffer = [];
  };
  for (const line of text.split("\n")) {
    const opener = FENCE.exec(line)?.[1];
    if (fence) {
      if (opener && opener[0] === fence[0] && opener.length >= fence.length)
        fence = null;
      buffer.push(line);
      continue;
    }
    if (opener) {
      fence = opener;
      buffer.push(line);
      continue;
    }
    const embed = EMBED.exec(line);
    if (!embed) {
      buffer.push(line);
      continue;
    }
    flush();
    parts.push({ kind: "visual", id: embed[2], caption: embed[1].trim() });
  }
  flush();
  return parts;
}

/** The ids a turn's replies already place in their text. */
export function placedVisualIds(blocks: readonly Block[]): Set<string> {
  const placed = new Set<string>();
  for (const block of blocks) {
    if (block.role !== "assistant") continue;
    for (const part of splitVisualEmbeds(block.text))
      if (part.kind === "visual") placed.add(part.id);
  }
  return placed;
}
