import type { Sheet } from "./spreadsheet";

/**
 * What a workbook draws over its cells: pictures and charts, read from the
 * file's drawing parts, which ExcelJS keeps only in part. Each lands on its
 * sheet with a pixel box measured from the cells it is anchored to.
 *
 * Runs in a worker, where there is no DOMParser, so the few XML parts needed
 * are read with the small parser below.
 */

export type DrawingBox = {
  x: number;
  y: number;
  width: number;
  height: number;
};

export type SheetImage = {
  kind: "image";
  box: DrawingBox;
  name: string;
  description?: string;
  /** The picture's bytes, as stored; the viewer checks what they are. */
  bytes: Uint8Array;
  /** Its extension in the file, for a format the viewer cannot draw (EMF). */
  extension: string;
};

export type ChartKind =
  "column" | "bar" | "line" | "area" | "pie" | "doughnut" | "scatter";

export type ChartSeries = {
  kind: ChartKind;
  name: string;
  values: (number | null)[];
  /** A scatter series' x values. */
  xValues?: (number | null)[];
  color?: string;
  /** One colour per point, for a pie. */
  pointColors?: (string | undefined)[];
};

export type SheetChart = {
  kind: "chart";
  box: DrawingBox;
  title?: string;
  categories: string[];
  series: ChartSeries[];
  stacking?: "stacked" | "percent";
  legend: boolean;
  xTitle?: string;
  yTitle?: string;
  /** A chart type this does not draw, by its XML name: shown as a card. */
  unsupported?: string;
};

export type SheetDrawing = SheetImage | SheetChart;

// ---------------------------------------------------------------- XML

export type XmlNode = {
  /** The local name: `c:barChart` → `barChart`. */
  name: string;
  attrs: Record<string, string>;
  children: XmlNode[];
  text: string;
};

function decodeEntities(text: string): string {
  return text.replace(
    /&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos);/gi,
    (_, code: string) => {
      const lower = code.toLowerCase();
      if (lower === "amp") return "&";
      if (lower === "lt") return "<";
      if (lower === "gt") return ">";
      if (lower === "quot") return '"';
      if (lower === "apos") return "'";
      const value = lower.startsWith("#x")
        ? parseInt(lower.slice(2), 16)
        : parseInt(lower.slice(1), 10);
      return Number.isFinite(value) ? String.fromCodePoint(value) : "";
    },
  );
}

const localName = (name: string) => name.slice(name.indexOf(":") + 1);

/** A tolerant reader for the well-formed XML Office writes. */
export function parseXml(xml: string): XmlNode {
  const root: XmlNode = { name: "#root", attrs: {}, children: [], text: "" };
  const stack = [root];
  const token =
    /<!--[\s\S]*?-->|<\?[\s\S]*?\?>|<!\[CDATA\[([\s\S]*?)\]\]>|<!DOCTYPE[^>]*>|<(\/?)([\w:.-]+)((?:\s+[\w:.-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>|([^<]+)/g;
  for (const match of xml.matchAll(token)) {
    const [, cdata, closing, tag, rawAttrs, selfClosing, text] = match;
    const parent = stack[stack.length - 1];
    if (cdata !== undefined) parent.text += cdata;
    else if (text !== undefined) parent.text += decodeEntities(text);
    else if (tag) {
      if (closing) {
        if (stack.length > 1) stack.pop();
        continue;
      }
      const attrs: Record<string, string> = {};
      for (const [, key, double, single] of (rawAttrs ?? "").matchAll(
        /([\w:.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g,
      )) {
        const value = decodeEntities(double ?? single ?? "");
        attrs[key] = value;
        attrs[localName(key)] ??= value;
      }
      const node: XmlNode = {
        name: localName(tag),
        attrs,
        children: [],
        text: "",
      };
      parent.children.push(node);
      if (!selfClosing) stack.push(node);
    }
  }
  return root;
}

export function child(
  node: XmlNode | undefined,
  name: string,
): XmlNode | undefined {
  return node?.children.find((entry) => entry.name === name);
}

function children(node: XmlNode | undefined, name: string): XmlNode[] {
  return node?.children.filter((entry) => entry.name === name) ?? [];
}

/** The first node by that name anywhere under this one. */
function find(node: XmlNode | undefined, name: string): XmlNode | undefined {
  if (!node) return undefined;
  for (const entry of node.children) {
    if (entry.name === name) return entry;
    const deeper = find(entry, name);
    if (deeper) return deeper;
  }
  return undefined;
}

function findAll(
  node: XmlNode | undefined,
  name: string,
  out: XmlNode[] = [],
): XmlNode[] {
  for (const entry of node?.children ?? []) {
    if (entry.name === name) out.push(entry);
    findAll(entry, name, out);
  }
  return out;
}

function path(
  node: XmlNode | undefined,
  ...names: string[]
): XmlNode | undefined {
  let current = node;
  for (const name of names) current = child(current, name);
  return current;
}

// ---------------------------------------------------------------- package parts

/** Where a relationship's target lives in the package. */
export function resolvePart(base: string, target: string): string {
  if (target.startsWith("/")) return target.slice(1);
  const parts = base.split("/").slice(0, -1);
  for (const piece of target.split("/")) {
    if (piece === "..") parts.pop();
    else if (piece && piece !== ".") parts.push(piece);
  }
  return parts.join("/");
}

function relsPath(part: string): string {
  const slash = part.lastIndexOf("/");
  return `${part.slice(0, slash + 1)}_rels/${part.slice(slash + 1)}.rels`;
}

type Rel = { target: string; type: string; external: boolean };

function readRels(xml: string | undefined): Map<string, Rel> {
  const rels = new Map<string, Rel>();
  if (!xml) return rels;
  for (const rel of findAll(parseXml(xml), "Relationship"))
    rels.set(rel.attrs.Id, {
      target: rel.attrs.Target ?? "",
      type: rel.attrs.Type ?? "",
      external: rel.attrs.TargetMode === "External",
    });
  return rels;
}

/** The package's files, as the drawing reader needs them. */
export type Package = {
  text(path: string): Promise<string | undefined>;
  bytes(path: string): Promise<Uint8Array | undefined>;
};

export async function zipPackage(bytes: Uint8Array): Promise<Package> {
  const module = (await import("jszip")) as unknown as {
    default?: { loadAsync(data: Uint8Array): Promise<Zip> };
    loadAsync?(data: Uint8Array): Promise<Zip>;
  };
  type Zip = {
    file(
      path: string,
    ): { async(kind: "string" | "uint8array"): Promise<unknown> } | null;
  };
  const load = module.loadAsync ?? module.default?.loadAsync;
  if (!load) throw new Error("The zip reader did not load.");
  const zip = await load.call(module.default ?? module, bytes);
  return {
    text: async (name) =>
      (await zip.file(name)?.async("string")) as string | undefined,
    bytes: async (name) =>
      (await zip.file(name)?.async("uint8array")) as Uint8Array | undefined,
  };
}

// ---------------------------------------------------------------- anchors

const EMU_PER_PIXEL = 9525;

type Marker = { col: number; colOff: number; row: number; rowOff: number };

function marker(node: XmlNode | undefined): Marker | undefined {
  if (!node) return undefined;
  const number = (name: string) =>
    Number(child(node, name)?.text.trim() ?? 0) || 0;
  return {
    col: number("col"),
    colOff: number("colOff") / EMU_PER_PIXEL,
    row: number("row"),
    rowOff: number("rowOff") / EMU_PER_PIXEL,
  };
}

function extent(
  node: XmlNode | undefined,
): { width: number; height: number } | undefined {
  if (!node) return undefined;
  return {
    width: Number(node.attrs.cx ?? 0) / EMU_PER_PIXEL,
    height: Number(node.attrs.cy ?? 0) / EMU_PER_PIXEL,
  };
}

const DEFAULT_COL = 64;
const DEFAULT_ROW = 20;

/** Grow a sheet's grid until it reaches `col`/`row`, so a drawing has cells under it. */
function reach(sheet: Sheet, col: number, row: number) {
  while (sheet.colWidths.length <= col) sheet.colWidths.push(DEFAULT_COL);
  while (sheet.rowHeights.length <= row) sheet.rowHeights.push(DEFAULT_ROW);
  sheet.colCount = sheet.colWidths.length;
  sheet.rowCount = sheet.rowHeights.length;
}

function offset(sizes: number[], index: number): number {
  let total = 0;
  for (let i = 0; i < index; i++) total += sizes[i] ?? 0;
  return total;
}

/** The index whose span holds a pixel position, growing the grid to reach it. */
function indexFor(
  sheet: Sheet,
  sizes: "colWidths" | "rowHeights",
  position: number,
): number {
  let total = 0;
  for (let index = 0; ; index++) {
    if (index >= sheet[sizes].length)
      reach(
        sheet,
        sizes === "colWidths" ? index : 0,
        sizes === "rowHeights" ? index : 0,
      );
    total += sheet[sizes][index];
    if (total > position || index > 20000) return index;
  }
}

/** The pixel box of an anchor, against the sheet's own column widths and row heights. */
export function anchorBox(
  anchor: XmlNode,
  sheet: Sheet,
): DrawingBox | undefined {
  const from = marker(child(anchor, "from"));
  if (anchor.name === "absoluteAnchor") {
    const pos = child(anchor, "pos");
    const size = extent(child(anchor, "ext"));
    if (!pos || !size) return undefined;
    const x = Number(pos.attrs.x ?? 0) / EMU_PER_PIXEL;
    const y = Number(pos.attrs.y ?? 0) / EMU_PER_PIXEL;
    indexFor(sheet, "colWidths", x + size.width);
    indexFor(sheet, "rowHeights", y + size.height);
    return { x, y, ...size };
  }
  if (!from) return undefined;
  reach(sheet, from.col + 1, from.row + 1);
  const x = offset(sheet.colWidths, from.col) + from.colOff;
  const y = offset(sheet.rowHeights, from.row) + from.rowOff;
  if (anchor.name === "oneCellAnchor") {
    const size = extent(child(anchor, "ext"));
    if (!size) return undefined;
    indexFor(sheet, "colWidths", x + size.width);
    indexFor(sheet, "rowHeights", y + size.height);
    return { x, y, ...size };
  }
  const to = marker(child(anchor, "to"));
  if (!to) return undefined;
  reach(sheet, to.col + 1, to.row + 1);
  const right = offset(sheet.colWidths, to.col) + to.colOff;
  const bottom = offset(sheet.rowHeights, to.row) + to.rowOff;
  return {
    x,
    y,
    width: Math.max(0, right - x),
    height: Math.max(0, bottom - y),
  };
}

// ---------------------------------------------------------------- charts

/** A range's values, as a chart reads them from its sheet. */
export type RangeReader = (formula: string) => {
  texts: string[];
  numbers: (number | null)[];
};

function richText(node: XmlNode | undefined): string | undefined {
  if (!node) return undefined;
  const runs = findAll(node, "t").map((t) => t.text);
  if (runs.length) return runs.join("");
  const cached = findAll(child(node, "strRef") ?? node, "v").map((v) => v.text);
  return cached.length ? cached.join(" ") : undefined;
}

function titleOf(
  node: XmlNode | undefined,
  read: RangeReader,
): string | undefined {
  const tx = child(node, "tx");
  if (!tx) return undefined;
  const reference = path(tx, "strRef", "f")?.text;
  return (
    richText(tx) ?? (reference ? read(reference).texts.join(" ") : undefined)
  );
}

/** A data reference's points: its cache when saved, else the cells it names. */
function points(
  node: XmlNode | undefined,
  read: RangeReader,
): { texts: string[]; numbers: (number | null)[] } {
  const ref =
    node &&
    (child(node, "numRef") ??
      child(node, "strRef") ??
      child(node, "multiLvlStrRef"));
  const literal = node && (child(node, "numLit") ?? child(node, "strLit"));
  const cache =
    (ref &&
      (child(ref, "numCache") ??
        child(ref, "strCache") ??
        child(ref, "multiLvlStrCache"))) ??
    literal;
  const pts = cache ? findAll(child(cache, "lvl") ?? cache, "pt") : [];
  if (pts.length) {
    const count = Number(child(cache, "ptCount")?.attrs.val ?? 0) || 0;
    const size = Math.max(
      count,
      ...pts.map((pt) => Number(pt.attrs.idx ?? 0) + 1),
    );
    const texts = Array.from({ length: size }, () => "");
    const numbers: (number | null)[] = Array.from({ length: size }, () => null);
    for (const pt of pts) {
      const index = Number(pt.attrs.idx ?? 0);
      const value = child(pt, "v")?.text ?? "";
      texts[index] = value;
      const number = Number(value);
      numbers[index] =
        value.trim() !== "" && Number.isFinite(number) ? number : null;
    }
    return { texts, numbers };
  }
  const formula = ref && child(ref, "f")?.text;
  return formula ? read(formula) : { texts: [], numbers: [] };
}

function schemeColor(
  name: string,
  theme: readonly string[],
): string | undefined {
  const index: Record<string, number> = {
    bg1: 0,
    lt1: 0,
    tx1: 1,
    dk1: 1,
    bg2: 2,
    lt2: 2,
    tx2: 3,
    dk2: 3,
    accent1: 4,
    accent2: 5,
    accent3: 6,
    accent4: 7,
    accent5: 8,
    accent6: 9,
  };
  return index[name] !== undefined ? theme[index[name]] : undefined;
}

function fillColor(
  spPr: XmlNode | undefined,
  theme: readonly string[],
  line = false,
): string | undefined {
  const holder = line ? child(spPr, "ln") : spPr;
  const solid = child(holder, "solidFill");
  if (!solid) return undefined;
  const rgb = child(solid, "srgbClr")?.attrs.val;
  if (rgb) return `#${rgb.toLowerCase()}`;
  const scheme = child(solid, "schemeClr")?.attrs.val;
  return scheme ? schemeColor(scheme, theme) : undefined;
}

const GROUP_KINDS: Record<string, ChartKind | undefined> = {
  barChart: "column",
  bar3DChart: "column",
  lineChart: "line",
  line3DChart: "line",
  areaChart: "area",
  area3DChart: "area",
  pieChart: "pie",
  pie3DChart: "pie",
  ofPieChart: "pie",
  doughnutChart: "doughnut",
  scatterChart: "scatter",
};

/** A chart part's drawing: its type, series, labels and colours. */
export function parseChart(
  xml: string,
  theme: readonly string[],
  read: RangeReader,
): Omit<SheetChart, "kind" | "box"> {
  const space = parseXml(xml);
  const chart = find(space, "chart");
  const plot = child(chart, "plotArea");
  const autoDeleted = child(chart, "autoTitleDeleted")?.attrs.val === "1";
  const series: ChartSeries[] = [];
  let categories: string[] = [];
  let stacking: SheetChart["stacking"];
  let unsupported: string | undefined;
  const accents = theme.slice(4, 10);
  for (const group of plot?.children ?? []) {
    if (!group.name.endsWith("Chart")) continue;
    let kind = GROUP_KINDS[group.name];
    if (!kind) {
      unsupported ??= group.name.replace(/Chart$/, "");
      continue;
    }
    if (kind === "column" && child(group, "barDir")?.attrs.val === "bar")
      kind = "bar";
    const grouping = child(group, "grouping")?.attrs.val;
    if (grouping === "stacked") stacking = "stacked";
    else if (grouping === "percentStacked") stacking = "percent";
    for (const ser of children(group, "ser")) {
      const index = series.length;
      const name = titleOf(ser, read) ?? `Series ${index + 1}`;
      const fallback = accents[index % accents.length];
      const spPr = child(ser, "spPr");
      const color =
        kind === "line" || kind === "scatter"
          ? (fillColor(spPr, theme, true) ?? fillColor(spPr, theme) ?? fallback)
          : (fillColor(spPr, theme) ?? fallback);
      if (kind === "scatter") {
        const x = points(child(ser, "xVal"), read);
        const y = points(child(ser, "yVal"), read);
        series.push({
          kind,
          name,
          values: y.numbers,
          xValues: x.numbers,
          color,
        });
        continue;
      }
      const cat = points(child(ser, "cat"), read);
      if (!categories.length && cat.texts.length) categories = cat.texts;
      const values = points(child(ser, "val"), read).numbers;
      const entry: ChartSeries = { kind, name, values, color };
      if (kind === "pie" || kind === "doughnut") {
        const custom = new Map(
          children(ser, "dPt").map((dPt) => [
            Number(child(dPt, "idx")?.attrs.val ?? -1),
            fillColor(child(dPt, "spPr"), theme),
          ]),
        );
        entry.pointColors = values.map(
          (_, point) => custom.get(point) ?? accents[point % accents.length],
        );
      }
      series.push(entry);
    }
  }
  if (!categories.length) {
    const longest = Math.max(0, ...series.map((entry) => entry.values.length));
    categories = Array.from({ length: longest }, (_, index) =>
      String(index + 1),
    );
  }
  // Each axis says where it sits: along the bottom it is x, up the side y.
  let xTitle: string | undefined;
  let yTitle: string | undefined;
  for (const axis of plot?.children ?? []) {
    if (!/^(catAx|valAx|dateAx|serAx)$/.test(axis.name)) continue;
    const title = titleOf(child(axis, "title"), read);
    if (!title) continue;
    const position = child(axis, "axPos")?.attrs.val;
    if (position === "b" || position === "t") xTitle ??= title;
    else yTitle ??= title;
  }
  const title =
    titleOf(child(chart, "title"), read) ??
    (child(chart, "title") && !autoDeleted && series.length === 1
      ? series[0].name
      : undefined);
  return {
    title,
    categories,
    series,
    stacking,
    legend: !!child(chart, "legend"),
    ...(xTitle ? { xTitle } : {}),
    ...(yTitle ? { yTitle } : {}),
    ...(unsupported && !series.length ? { unsupported } : {}),
  };
}

// ---------------------------------------------------------------- reading a workbook's drawings

function decodeSheetName(name: string): string {
  return name.replace(/^'(.*)'$/, "$1").replace(/''/g, "'");
}

/**
 * Read `'Hoja 1'!$B$2:$B$5` (or a union of such ranges) from the sheets, row
 * by row: the texts they print and the numbers behind them.
 */
export function rangeReader(sheets: ReadonlyMap<string, Sheet>): RangeReader {
  const indexes = new Map<Sheet, Map<number, Sheet["cells"][number]>>();
  const cellsOf = (sheet: Sheet) => {
    let index = indexes.get(sheet);
    if (!index) {
      index = new Map(
        sheet.cells.map((cell) => [cell.row * 16384 + cell.col, cell]),
      );
      indexes.set(sheet, index);
    }
    return index;
  };
  const column = (letters: string) =>
    [...letters.toUpperCase()].reduce(
      (total, char) => total * 26 + char.charCodeAt(0) - 64,
      0,
    ) - 1;
  return (formula) => {
    const texts: string[] = [];
    const numbers: (number | null)[] = [];
    for (const part of formula
      .replace(/^\(|\)$/g, "")
      .split(/,(?=(?:[^']*'[^']*')*[^']*$)/)) {
      const match =
        /^(?:('(?:[^']|'')+'|[^!]+)!)?\$?([A-Z]+)\$?(\d+)(?::\$?([A-Z]+)\$?(\d+))?$/i.exec(
          part.trim(),
        );
      if (!match) continue;
      const sheet =
        sheets.get(decodeSheetName(match[1] ?? "")) ?? [...sheets.values()][0];
      if (!sheet) continue;
      const cells = cellsOf(sheet);
      const [c1, r1] = [column(match[2]), Number(match[3]) - 1];
      const [c2, r2] = match[4]
        ? [column(match[4]), Number(match[5]) - 1]
        : [c1, r1];
      for (let row = Math.min(r1, r2); row <= Math.max(r1, r2); row++)
        for (let col = Math.min(c1, c2); col <= Math.max(c1, c2); col++) {
          const cell = cells.get(row * 16384 + col);
          texts.push(cell?.text ?? "");
          numbers.push(cell?.value ?? null);
        }
    }
    return { texts, numbers };
  };
}

/**
 * Every visible sheet's pictures and charts. A part that cannot be read is
 * skipped: the cells still show.
 */
export async function readDrawings(
  pkg: Package,
  sheets: ReadonlyMap<string, Sheet>,
  theme: readonly string[],
): Promise<void> {
  const workbookXml = await pkg.text("xl/workbook.xml");
  if (!workbookXml) return;
  const workbookRels = readRels(await pkg.text(relsPath("xl/workbook.xml")));
  const read = rangeReader(sheets);
  for (const entry of findAll(parseXml(workbookXml), "sheet")) {
    const sheet = sheets.get(entry.attrs.name ?? "");
    const rel = workbookRels.get(entry.attrs["r:id"] ?? entry.attrs.id ?? "");
    if (!sheet || !rel) continue;
    const sheetPart = resolvePart("xl/workbook.xml", rel.target);
    const sheetRels = readRels(await pkg.text(relsPath(sheetPart)));
    const drawings: SheetDrawing[] = [];
    for (const drawingRel of sheetRels.values()) {
      if (!drawingRel.type.endsWith("/drawing") || drawingRel.external)
        continue;
      const drawingPart = resolvePart(sheetPart, drawingRel.target);
      const drawingXml = await pkg.text(drawingPart);
      if (!drawingXml) continue;
      const rels = readRels(await pkg.text(relsPath(drawingPart)));
      const root = parseXml(drawingXml);
      const anchors = find(root, "wsDr")?.children ?? root.children;
      for (const anchor of anchors) {
        if (!/^(twoCellAnchor|oneCellAnchor|absoluteAnchor)$/.test(anchor.name))
          continue;
        try {
          const drawing = await readAnchor(
            anchor,
            sheet,
            drawingPart,
            rels,
            pkg,
            theme,
            read,
          );
          if (drawing) drawings.push(drawing);
        } catch {
          // One unreadable picture or chart leaves the rest.
        }
      }
    }
    if (drawings.length) sheet.drawings = drawings;
  }
}

async function readAnchor(
  anchor: XmlNode,
  sheet: Sheet,
  drawingPart: string,
  rels: Map<string, Rel>,
  pkg: Package,
  theme: readonly string[],
  read: RangeReader,
): Promise<SheetDrawing | undefined> {
  const pic = find(anchor, "pic");
  const frame = find(anchor, "graphicFrame");
  if (pic) {
    const embed =
      find(pic, "blip")?.attrs["r:embed"] ?? find(pic, "blip")?.attrs.embed;
    const rel = embed ? rels.get(embed) : undefined;
    if (!rel || rel.external) return undefined;
    const part = resolvePart(drawingPart, rel.target);
    const bytes = await pkg.bytes(part);
    const box = anchorBox(anchor, sheet);
    if (!bytes || !box) return undefined;
    const props = find(pic, "cNvPr");
    return {
      kind: "image",
      box,
      name: props?.attrs.name ?? part.split("/").pop() ?? "Picture",
      ...(props?.attrs.descr ? { description: props.attrs.descr } : {}),
      bytes,
      extension: (part.split(".").pop() ?? "").toLowerCase(),
    };
  }
  if (frame) {
    const chartRef = find(frame, "chart");
    const id = chartRef?.attrs["r:id"] ?? chartRef?.attrs.id;
    const rel = id ? rels.get(id) : undefined;
    if (!rel || rel.external) return undefined;
    const xml = await pkg.text(resolvePart(drawingPart, rel.target));
    const box = anchorBox(anchor, sheet);
    if (!xml || !box) return undefined;
    return { kind: "chart", box, ...parseChart(xml, theme, read) };
  }
  return undefined;
}
