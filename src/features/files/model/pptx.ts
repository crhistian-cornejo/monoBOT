import JSZip from "jszip";

/**
 * A PowerPoint deck read for showing, not editing: each slide's boxes where
 * the file puts them (in EMU, as the file measures), their text with its
 * sizes and colors, pictures, tables and the background. What it cannot
 * draw (charts, SmartArt, video) shows as a labelled box.
 */
export type Deck = { width: number; height: number; slides: Slide[] };

export type Slide = {
  number: number;
  background?: string;
  backgroundImage?: string;
  items: SlideItem[];
};

export type Frame = {
  x: number;
  y: number;
  w: number;
  h: number;
  rotate?: number;
  flipH?: boolean;
  flipV?: boolean;
};

export type Run = {
  text: string;
  /** Points. */
  size: number;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  color?: string;
};

export type Paragraph = {
  runs: Run[];
  align: "left" | "center" | "right" | "justify";
  level: number;
  bullet?: string;
  /** Points, for an empty line's height. */
  size: number;
};

export type TextBody = {
  paragraphs: Paragraph[];
  anchor: "top" | "middle" | "bottom";
  /** Top, right, bottom, left, in EMU. */
  inset: [number, number, number, number];
};

export type TableCell = { span: number; fill?: string; text: TextBody };

export type SlideItem =
  | (Frame & {
      kind: "shape";
      fill?: string;
      line?: string;
      shape?: "ellipse" | "round";
      text?: TextBody;
    })
  | (Frame & { kind: "image"; src: string })
  | (Frame & {
      kind: "table";
      columns: number[];
      rows: { height: number; cells: TableCell[] }[];
    })
  | (Frame & { kind: "unsupported"; label: string });

/** Past this many slides the rest are left out; the app opens it whole. */
export const MAX_SLIDES = 300;
/** A picture larger than this is left out rather than inlined. */
const MAX_IMAGE_BYTES = 15 * 1024 * 1024;
const EMU_PER_PT = 12700;
const DEFAULT_SIZE = 18;

const IMAGE_TYPES: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  bmp: "image/bmp",
  webp: "image/webp",
  svg: "image/svg+xml",
};

// ---- XML, by local name: the prefixes (p:, a:, r:) vary between writers.

function children(el: Element | undefined, name?: string): Element[] {
  if (!el) return [];
  return [...el.children].filter((child) => !name || child.localName === name);
}

function child(el: Element | undefined, ...path: string[]): Element | undefined {
  let at = el;
  for (const name of path) {
    at = children(at, name)[0];
    if (!at) return undefined;
  }
  return at;
}

function descendant(el: Element | undefined, name: string): Element | undefined {
  if (!el) return undefined;
  for (const entry of children(el)) {
    if (entry.localName === name) return entry;
    const found = descendant(entry, name);
    if (found) return found;
  }
  return undefined;
}

function attr(el: Element | undefined, name: string): string | undefined {
  if (!el) return undefined;
  const direct = el.getAttribute(name);
  if (direct != null) return direct;
  // `r:embed`, `r:id`: whatever the prefix is called.
  for (const entry of [...el.attributes])
    if (entry.localName === name || entry.name.endsWith(`:${name}`))
      return entry.value;
  return undefined;
}

/**
 * A relationship attribute (`r:id`, `r:embed`), never a plain one by its
 * name: `<p:sldId id="256" r:id="rId2">` holds both. parseXml renames them.
 */
function relAttr(el: Element | undefined, name: string): string | undefined {
  return el?.getAttribute(`rel_${name}`) ?? undefined;
}

function num(value: string | undefined): number | undefined {
  if (value == null || value === "") return undefined;
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function parseXml(text: string): Element | undefined {
  // Some parsers keep one attribute per local name, so `r:id` would be lost
  // beside `id`: relationship attributes get names of their own first.
  const named = text.replace(
    /(\s)(?!xmlns:)[A-Za-z][\w.-]*:(id|embed|link)=/g,
    "$1rel_$2=",
  );
  const doc = new DOMParser().parseFromString(named, "application/xml");
  const root = doc.documentElement;
  return root && root.localName !== "parsererror" ? root : undefined;
}

// ---- Parts and their relationships.

function dirOf(path: string): string {
  const at = path.lastIndexOf("/");
  return at < 0 ? "" : path.slice(0, at + 1);
}

/** A relationship's target as a path in the package. */
export function resolvePart(from: string, target: string): string {
  if (target.startsWith("/")) return target.slice(1);
  const parts = `${dirOf(from)}${target}`.split("/");
  const out: string[] = [];
  for (const part of parts) {
    if (part === "..") out.pop();
    else if (part && part !== ".") out.push(part);
  }
  return out.join("/");
}

function relsPath(part: string): string {
  return `${dirOf(part)}_rels/${part.slice(dirOf(part).length)}.rels`;
}

type Rel = { type: string; target: string };

class Package {
  private xml = new Map<string, Promise<Element | undefined>>();
  private rels = new Map<string, Promise<Map<string, Rel>>>();
  constructor(private zip: JSZip) {}

  part(path: string): Promise<Element | undefined> {
    let found = this.xml.get(path);
    if (!found) {
      const file = this.zip.file(path);
      found = file
        ? file.async("string").then(parseXml, () => undefined)
        : Promise.resolve(undefined);
      this.xml.set(path, found);
    }
    return found;
  }

  relationships(path: string): Promise<Map<string, Rel>> {
    let found = this.rels.get(path);
    if (!found) {
      found = this.part(relsPath(path)).then((root) => {
        const map = new Map<string, Rel>();
        for (const rel of children(root, "Relationship")) {
          const id = attr(rel, "Id");
          const target = attr(rel, "Target");
          if (!id || !target || attr(rel, "TargetMode") === "External")
            continue;
          map.set(id, {
            type: (attr(rel, "Type") ?? "").split("/").pop() ?? "",
            target: resolvePart(path, target),
          });
        }
        return map;
      });
      this.rels.set(path, found);
    }
    return found;
  }

  async related(path: string, type: string): Promise<string | undefined> {
    for (const rel of (await this.relationships(path)).values())
      if (rel.type === type) return rel.target;
    return undefined;
  }

  async image(path: string): Promise<string | undefined> {
    const extension = path.split(".").pop()?.toLowerCase() ?? "";
    const mime = IMAGE_TYPES[extension];
    const file = this.zip.file(path);
    if (!mime || !file) return undefined;
    const bytes = await file.async("uint8array");
    if (bytes.length > MAX_IMAGE_BYTES) return undefined;
    return `data:${mime};base64,${toBase64(bytes)}`;
  }
}

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let at = 0; at < bytes.length; at += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(binary);
}

// ---- Colors.

type Theme = Map<string, string>;

const SCHEME_ALIAS: Record<string, string> = {
  tx1: "dk1",
  tx2: "dk2",
  bg1: "lt1",
  bg2: "lt2",
};

function readTheme(root: Element | undefined): Theme {
  const theme: Theme = new Map();
  const scheme = descendant(root, "clrScheme");
  for (const entry of children(scheme)) {
    const value =
      attr(child(entry, "srgbClr"), "val") ??
      attr(child(entry, "sysClr"), "lastClr");
    if (value) theme.set(entry.localName, `#${value}`);
  }
  return theme;
}

function hexToRgb(hex: string): [number, number, number] {
  const value = Number.parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

function rgbToHex([r, g, b]: [number, number, number]): string {
  return `#${[r, g, b]
    .map((part) =>
      Math.max(0, Math.min(255, Math.round(part)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/** lumMod/lumOff/tint/shade, roughly as Office applies them. */
function modulate(hex: string, el: Element): string {
  let [r, g, b] = hexToRgb(hex);
  for (const step of children(el)) {
    const value = (num(attr(step, "val")) ?? 100000) / 100000;
    if (step.localName === "lumMod") [r, g, b] = [r * value, g * value, b * value];
    else if (step.localName === "lumOff")
      [r, g, b] = [r + 255 * value, g + 255 * value, b + 255 * value];
    else if (step.localName === "tint")
      [r, g, b] = [r, g, b].map((c) => c + (255 - c) * (1 - value)) as [
        number,
        number,
        number,
      ];
    else if (step.localName === "shade")
      [r, g, b] = [r * value, g * value, b * value];
  }
  return rgbToHex([r, g, b]);
}

/** The color of a fill-like element (solidFill, a run's color…). */
function colorOf(el: Element | undefined, theme: Theme): string | undefined {
  if (!el) return undefined;
  for (const entry of children(el)) {
    let hex: string | undefined;
    if (entry.localName === "srgbClr") hex = `#${attr(entry, "val") ?? ""}`;
    else if (entry.localName === "sysClr")
      hex = attr(entry, "lastClr") ? `#${attr(entry, "lastClr")}` : undefined;
    else if (entry.localName === "schemeClr") {
      const name = attr(entry, "val") ?? "";
      hex = theme.get(SCHEME_ALIAS[name] ?? name);
    } else if (entry.localName === "prstClr")
      hex = attr(entry, "val") === "white" ? "#FFFFFF" : "#000000";
    if (hex && /^#[0-9a-f]{6}$/i.test(hex)) return modulate(hex, entry);
  }
  return undefined;
}

function fillOf(spPr: Element | undefined, theme: Theme): string | undefined {
  return colorOf(child(spPr, "solidFill"), theme);
}

// ---- Placeholders and inherited text styles.

type Placeholder = { type: string; idx?: string };

function placeholderOf(shape: Element): Placeholder | undefined {
  const nv =
    child(shape, "nvSpPr", "nvPr") ??
    child(shape, "nvPicPr", "nvPr") ??
    child(shape, "nvGraphicFramePr", "nvPr");
  const ph = child(nv, "ph");
  if (!ph) return undefined;
  const idx = attr(ph, "idx");
  return { type: attr(ph, "type") ?? "body", ...(idx ? { idx } : {}) };
}

function findPlaceholder(
  tree: Element | undefined,
  wanted: Placeholder,
): Element | undefined {
  const shapes = children(tree).filter((el) => el.localName === "sp");
  const typed = (ph: Placeholder) =>
    ph.type === wanted.type ||
    (wanted.type === "ctrTitle" && ph.type === "title") ||
    (wanted.type === "subTitle" && ph.type === "body");
  return (
    (wanted.idx
      ? shapes.find((shape) => placeholderOf(shape)?.idx === wanted.idx)
      : undefined) ??
    shapes.find((shape) => {
      const ph = placeholderOf(shape);
      return !!ph && typed(ph);
    })
  );
}

/** Where a slide's text and placeholders inherit from. */
type Context = {
  theme: Theme;
  layoutTree?: Element;
  masterTree?: Element;
  masterStyles?: Element;
};

function xfrmOf(shape: Element | undefined): Element | undefined {
  return (
    child(shape, "spPr", "xfrm") ??
    child(shape, "grpSpPr", "xfrm") ??
    child(shape, "xfrm")
  );
}

function frameOf(xfrm: Element | undefined): Frame | undefined {
  const off = child(xfrm, "off");
  const ext = child(xfrm, "ext");
  if (!off || !ext) return undefined;
  const rot = num(attr(xfrm, "rot"));
  return {
    x: num(attr(off, "x")) ?? 0,
    y: num(attr(off, "y")) ?? 0,
    w: num(attr(ext, "cx")) ?? 0,
    h: num(attr(ext, "cy")) ?? 0,
    ...(rot ? { rotate: rot / 60000 } : {}),
    ...(attr(xfrm, "flipH") === "1" ? { flipH: true } : {}),
    ...(attr(xfrm, "flipV") === "1" ? { flipV: true } : {}),
  };
}

/** The shapes this one inherits from: its layout's, then its master's. */
function inherited(shape: Element, context: Context): Element[] {
  const ph = placeholderOf(shape);
  if (!ph) return [];
  return [
    findPlaceholder(child(context.layoutTree, "cSld", "spTree"), ph),
    findPlaceholder(child(context.masterTree, "cSld", "spTree"), ph),
  ].filter((entry): entry is Element => !!entry);
}

function masterTextStyle(ph: Placeholder | undefined, context: Context) {
  const styles = context.masterStyles;
  if (!ph) return child(styles, "otherStyle");
  return ph.type === "title" || ph.type === "ctrTitle"
    ? child(styles, "titleStyle")
    : child(styles, "bodyStyle");
}

// ---- Text.

const ALIGN: Record<string, Paragraph["align"]> = {
  l: "left",
  ctr: "center",
  r: "right",
  just: "justify",
  dist: "justify",
};

function textBodyOf(
  body: Element | undefined,
  lists: (Element | undefined)[],
  bodyProps: (Element | undefined)[],
  context: Context,
  bulletsByDefault: boolean,
): TextBody | undefined {
  if (!body) return undefined;
  const props = [child(body, "bodyPr"), ...bodyProps];
  const pick = (name: string) => {
    for (const entry of props) {
      const value = attr(entry, name);
      if (value != null) return value;
    }
    return undefined;
  };
  const autofit = descendant(child(body, "bodyPr"), "normAutofit");
  const scale = (num(attr(autofit, "fontScale")) ?? 100000) / 100000;
  const levelProps = (level: number) =>
    [child(body, "lstStyle"), ...lists].map((list) =>
      child(list, `lvl${level + 1}pPr`),
    );
  const paragraphs = children(body, "p").map((p): Paragraph => {
    const pPr = child(p, "pPr");
    const level = num(attr(pPr, "lvl")) ?? 0;
    const chain = [pPr, ...levelProps(level)];
    const fromChain = <T,>(read: (el: Element) => T | undefined) => {
      for (const entry of chain) {
        const value = entry ? read(entry) : undefined;
        if (value !== undefined) return value;
      }
      return undefined;
    };
    const defaultSize =
      (fromChain((el) => num(attr(child(el, "defRPr"), "sz"))) ??
        DEFAULT_SIZE * 100) / 100;
    const defaultColor = fromChain((el) =>
      colorOf(child(el, "defRPr", "solidFill"), context.theme),
    );
    const defaultBold = fromChain((el) => attr(child(el, "defRPr"), "b"));
    const runs: Run[] = [];
    for (const entry of children(p)) {
      if (entry.localName === "br") {
        runs.push({ text: "\n", size: defaultSize * scale });
        continue;
      }
      if (entry.localName !== "r" && entry.localName !== "fld") continue;
      const text = child(entry, "t")?.textContent ?? "";
      if (!text) continue;
      const rPr = child(entry, "rPr");
      const size = (num(attr(rPr, "sz")) ?? defaultSize * 100) / 100;
      const color = colorOf(child(rPr, "solidFill"), context.theme) ?? defaultColor;
      const bold = (attr(rPr, "b") ?? defaultBold) === "1";
      runs.push({
        text,
        size: size * scale,
        ...(bold ? { bold } : {}),
        ...(attr(rPr, "i") === "1" ? { italic: true } : {}),
        ...(attr(rPr, "u") && attr(rPr, "u") !== "none"
          ? { underline: true }
          : {}),
        ...(color ? { color } : {}),
      });
    }
    const bulletChar = fromChain((el) =>
      child(el, "buNone")
        ? ""
        : (attr(child(el, "buChar"), "char") ??
          (child(el, "buAutoNum") ? "•" : undefined)),
    );
    const bullet =
      runs.length && bulletChar !== ""
        ? (bulletChar ?? (bulletsByDefault ? "•" : undefined))
        : undefined;
    return {
      runs,
      align: ALIGN[fromChain((el) => attr(el, "algn")) ?? "l"] ?? "left",
      level,
      ...(bullet ? { bullet } : {}),
      size: defaultSize * scale,
    };
  });
  const anchor = pick("anchor");
  return {
    paragraphs,
    anchor: anchor === "ctr" ? "middle" : anchor === "b" ? "bottom" : "top",
    inset: [
      num(pick("tIns")) ?? 45720,
      num(pick("rIns")) ?? 91440,
      num(pick("bIns")) ?? 45720,
      num(pick("lIns")) ?? 91440,
    ],
  };
}

// ---- Shapes.

type Mapping = (frame: Frame) => Frame;
const IDENTITY: Mapping = (frame) => frame;

async function readShape(
  shape: Element,
  pkg: Package,
  slidePath: string,
  context: Context,
  map: Mapping,
): Promise<SlideItem[]> {
  switch (shape.localName) {
    case "sp": {
      const sources = inherited(shape, context);
      const raw =
        frameOf(xfrmOf(shape)) ??
        sources.map((entry) => frameOf(xfrmOf(entry))).find(Boolean);
      if (!raw) return [];
      const ph = placeholderOf(shape);
      const spPr = child(shape, "spPr");
      const fill = child(spPr, "noFill")
        ? undefined
        : (fillOf(spPr, context.theme) ??
          sources.map((entry) => fillOf(child(entry, "spPr"), context.theme)).find(Boolean));
      const line = child(spPr, "ln", "noFill")
        ? undefined
        : colorOf(child(spPr, "ln", "solidFill"), context.theme);
      const geometry = attr(child(spPr, "prstGeom"), "prst");
      const text = textBodyOf(
        child(shape, "txBody"),
        [
          ...sources.map((entry) => child(entry, "txBody", "lstStyle")),
          masterTextStyle(ph, context),
        ],
        sources.map((entry) => child(entry, "txBody", "bodyPr")),
        context,
        !!ph && ph.type !== "title" && ph.type !== "ctrTitle" && ph.type !== "subTitle",
      );
      const hasText = !!text?.paragraphs.some((p) => p.runs.length);
      if (!fill && !line && !hasText) return [];
      return [
        {
          kind: "shape",
          ...map(raw),
          ...(fill ? { fill } : {}),
          ...(line ? { line } : {}),
          ...(geometry === "ellipse"
            ? { shape: "ellipse" as const }
            : geometry === "roundRect"
              ? { shape: "round" as const }
              : {}),
          ...(text && hasText ? { text } : {}),
        },
      ];
    }
    case "pic": {
      const raw =
        frameOf(xfrmOf(shape)) ??
        inherited(shape, context)
          .map((entry) => frameOf(xfrmOf(entry)))
          .find(Boolean);
      const embed = relAttr(descendant(child(shape, "blipFill"), "blip"), "embed");
      const rel = embed
        ? (await pkg.relationships(slidePath)).get(embed)
        : undefined;
      const src = rel ? await pkg.image(rel.target) : undefined;
      if (!raw) return [];
      return src
        ? [{ kind: "image", ...map(raw), src }]
        : [{ kind: "unsupported", ...map(raw), label: "Picture" }];
    }
    case "graphicFrame": {
      const raw = frameOf(child(shape, "xfrm"));
      if (!raw) return [];
      const table = descendant(shape, "tbl");
      if (!table) {
        const uri = attr(descendant(shape, "graphicData"), "uri") ?? "";
        const label = uri.includes("chart")
          ? "Chart"
          : uri.includes("diagram")
            ? "Diagram"
            : "Object";
        return [{ kind: "unsupported", ...map(raw), label }];
      }
      const frame = map(raw);
      const ratio = raw.w ? frame.w / raw.w : 1;
      return [
        {
          kind: "table",
          ...frame,
          columns: children(child(table, "tblGrid"), "gridCol").map(
            (col) => (num(attr(col, "w")) ?? 0) * ratio,
          ),
          rows: children(table, "tr").map((row) => ({
            height: (num(attr(row, "h")) ?? 0) * ratio,
            cells: children(row, "tc")
              .filter(
                (cell) => attr(cell, "hMerge") !== "1" && attr(cell, "vMerge") !== "1",
              )
              .map((cell) => {
                const tcPr = child(cell, "tcPr");
                const fill = fillOf(tcPr, context.theme);
                return {
                  span: num(attr(cell, "gridSpan")) ?? 1,
                  ...(fill ? { fill } : {}),
                  text: textBodyOf(
                    child(cell, "txBody"),
                    [child(context.masterStyles, "otherStyle")],
                    [],
                    context,
                    false,
                  ) ?? { paragraphs: [], anchor: "top", inset: [45720, 91440, 45720, 91440] },
                };
              }),
          })),
        },
      ];
    }
    case "grpSp": {
      const xfrm = child(shape, "grpSpPr", "xfrm");
      const outer = frameOf(xfrm);
      const chOff = child(xfrm, "chOff");
      const chExt = child(xfrm, "chExt");
      const inner = {
        x: num(attr(chOff, "x")) ?? 0,
        y: num(attr(chOff, "y")) ?? 0,
        w: num(attr(chExt, "cx")) ?? 0,
        h: num(attr(chExt, "cy")) ?? 0,
      };
      const sx = outer && inner.w ? outer.w / inner.w : 1;
      const sy = outer && inner.h ? outer.h / inner.h : 1;
      const groupMap: Mapping = outer
        ? (frame) =>
            map({
              ...frame,
              x: outer.x + (frame.x - inner.x) * sx,
              y: outer.y + (frame.y - inner.y) * sy,
              w: frame.w * sx,
              h: frame.h * sy,
            })
        : map;
      return readTree(shape, pkg, slidePath, context, groupMap);
    }
    case "AlternateContent": {
      // Newer content with a fallback older readers draw.
      const fallback = child(shape, "Fallback");
      return readTree(fallback, pkg, slidePath, context, map);
    }
    default:
      return [];
  }
}

async function readTree(
  tree: Element | undefined,
  pkg: Package,
  slidePath: string,
  context: Context,
  map: Mapping = IDENTITY,
  /** A layout's placeholders hold prompt text ("Click to add title"). */
  skipPlaceholders = false,
): Promise<SlideItem[]> {
  const items: SlideItem[] = [];
  for (const shape of children(tree)) {
    if (skipPlaceholders && placeholderOf(shape)) continue;
    items.push(...(await readShape(shape, pkg, slidePath, context, map)));
  }
  return items;
}

async function backgroundOf(
  roots: { root?: Element; path: string }[],
  pkg: Package,
  theme: Theme,
): Promise<{ color?: string; image?: string }> {
  for (const { root, path } of roots) {
    const bg = child(root, "cSld", "bg");
    if (!bg) continue;
    const bgPr = child(bg, "bgPr");
    const color =
      colorOf(child(bgPr, "solidFill"), theme) ??
      colorOf(child(bg, "bgRef"), theme) ??
      colorOf(descendant(child(bgPr, "gradFill"), "gs"), theme);
    const embed = relAttr(descendant(child(bgPr, "blipFill"), "blip"), "embed");
    const rel = embed ? (await pkg.relationships(path)).get(embed) : undefined;
    const image = rel ? await pkg.image(rel.target) : undefined;
    if (color || image)
      return { ...(color ? { color } : {}), ...(image ? { image } : {}) };
  }
  return {};
}

/** Reads a .pptx into its slides, in the order the deck shows them. */
export async function readPptx(bytes: Uint8Array): Promise<Deck> {
  const pkg = new Package(await JSZip.loadAsync(bytes));
  const presentationPath = "ppt/presentation.xml";
  const presentation = await pkg.part(presentationPath);
  if (!presentation) throw new Error("This is not a PowerPoint deck.");
  const size = child(presentation, "sldSz");
  const width = num(attr(size, "cx")) ?? 12192000;
  const height = num(attr(size, "cy")) ?? 6858000;
  const rels = await pkg.relationships(presentationPath);
  const slidePaths = children(child(presentation, "sldIdLst"), "sldId")
    .map((entry) => rels.get(relAttr(entry, "id") ?? "")?.target)
    .filter((path): path is string => !!path)
    .slice(0, MAX_SLIDES);
  const slides: Slide[] = [];
  for (const [index, path] of slidePaths.entries()) {
    const root = await pkg.part(path);
    if (!root || attr(root, "show") === "0") continue;
    const layoutPath = await pkg.related(path, "slideLayout");
    const layout = layoutPath ? await pkg.part(layoutPath) : undefined;
    const masterPath = layoutPath
      ? await pkg.related(layoutPath, "slideMaster")
      : undefined;
    const master = masterPath ? await pkg.part(masterPath) : undefined;
    const themePath = masterPath
      ? await pkg.related(masterPath, "theme")
      : undefined;
    const theme = readTheme(themePath ? await pkg.part(themePath) : undefined);
    const context: Context = {
      theme,
      ...(layout ? { layoutTree: layout } : {}),
      ...(master ? { masterTree: master } : {}),
      ...(child(master, "txStyles") ? { masterStyles: child(master, "txStyles") } : {}),
    };
    const background = await backgroundOf(
      [
        { root, path },
        ...(layout && layoutPath ? [{ root: layout, path: layoutPath }] : []),
        ...(master && masterPath ? [{ root: master, path: masterPath }] : []),
      ],
      pkg,
      theme,
    );
    // A layout's own pictures and shapes (logos, bands) sit under the slide's,
    // unless the slide says to hide them.
    const fromLayout =
      layout && layoutPath && attr(root, "showMasterSp") !== "0"
        ? (
            await readTree(
              child(layout, "cSld", "spTree"),
              pkg,
              layoutPath,
              { theme, ...(master ? { masterTree: master } : {}) },
              IDENTITY,
              true,
            )
          ).filter((item) => item.kind !== "unsupported")
        : [];
    const own = await readTree(child(root, "cSld", "spTree"), pkg, path, context);
    slides.push({
      number: index + 1,
      ...(background.color ? { background: background.color } : {}),
      ...(background.image ? { backgroundImage: background.image } : {}),
      items: [...fromLayout, ...own],
    });
  }
  return { width, height, slides };
}

/** Points to EMU, for a run's size on a slide drawn at some scale. */
export function pointsToEmu(points: number): number {
  return points * EMU_PER_PT;
}
