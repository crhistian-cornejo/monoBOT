import { pathSegments } from "./fileName";
import {
  readDrawings,
  zipPackage,
  type SheetDrawing,
} from "./spreadsheetDrawings";

/**
 * A spreadsheet read for showing, not editing: each sheet's cells with the
 * text Excel would print, the input the formula bar shows, and the look the
 * file gave them (fills, fonts, borders, merges, sizes, frozen panes).
 *
 * `.xlsx` and `.xlsm` are read with ExcelJS, loaded only when one opens; `.csv`
 * and `.tsv` are split here. The old binary `.xls` routes to the viewer too,
 * which offers to open it in Excel instead of showing its bytes as text.
 */

/** The file's name. Kept free of the Tauri bridge, so the worker can load this. */
function basename(path: string): string {
  return pathSegments(path).pop() ?? path;
}

export type CellBorder = {
  width: number;
  style: "solid" | "dashed" | "dotted" | "double";
  color?: string;
};

export type CellStyle = {
  /** `#rrggbb`. Absent: the theme's background. */
  fill?: string;
  /** `#rrggbb`. Absent: automatic, readable on the fill or the theme. */
  color?: string;
  bold?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
  /** Font size in points. */
  size?: number;
  font?: string;
  align?: "left" | "center" | "right" | "justify";
  valign?: "top" | "middle" | "bottom";
  wrap?: boolean;
  indent?: number;
  top?: CellBorder;
  right?: CellBorder;
  bottom?: CellBorder;
  left?: CellBorder;
};

export type SheetCell = {
  /** Zero-based. */
  row: number;
  col: number;
  /** What the cell prints. */
  text: string;
  /** What the formula bar shows: the formula, or the raw value. */
  input: string;
  /** Numbers and dates lean right, as in Excel, unless styled otherwise. */
  numeric?: boolean;
  /** The number behind a numeric cell, for the charts that read it. */
  value?: number;
  /** A formula saved without its result: the text is the formula itself. */
  uncalculated?: boolean;
  error?: boolean;
  rowSpan?: number;
  colSpan?: number;
  style?: CellStyle;
};

export type Sheet = {
  name: string;
  rowCount: number;
  colCount: number;
  /** Pixels; 0 for a hidden column or row. */
  colWidths: number[];
  rowHeights: number[];
  cells: SheetCell[];
  frozenRows: number;
  frozenCols: number;
  gridlines: boolean;
  /** The sheet had more cells than the viewer reads. */
  truncated: boolean;
  /** Pictures and charts over the cells, in the order the file draws them. */
  drawings?: SheetDrawing[];
};

export type Workbook = { sheets: Sheet[] };

/** Past this many cells a sheet is cut short; the rest is in Excel. */
export const MAX_CELLS = 250_000;

const DEFAULT_COL_WIDTH = 8.43;
const DEFAULT_ROW_HEIGHT = 15;
/** Empty rows and columns past the data, so the grid reads as a sheet. */
const PAD_ROWS = 20;
const PAD_COLS = 4;
const MIN_ROWS = 40;
const MIN_COLS = 12;

const EXTENSIONS = new Set([".xlsx", ".xlsm", ".xls", ".csv", ".tsv"]);

function extensionOf(path: string): string {
  const name = basename(path).toLowerCase();
  return name.includes(".") ? name.slice(name.lastIndexOf(".")) : "";
}

/** Whether a path opens in the spreadsheet viewer rather than the editor. */
export function isSpreadsheetPath(path: string): boolean {
  return EXTENSIONS.has(extensionOf(path));
}

/**
 * An Excel workbook: binary, so the editor has nothing to show. CSV stays
 * text there, where it can be edited; the agents' panel shows it as a sheet.
 */
export function isWorkbookPath(path: string): boolean {
  return [".xlsx", ".xlsm", ".xls"].includes(extensionOf(path));
}

/** The legacy binary format, which only Excel itself opens. */
export function isLegacyXls(path: string): boolean {
  return extensionOf(path) === ".xls";
}

/** 0 → A, 25 → Z, 26 → AA. */
export function columnName(index: number): string {
  let name = "";
  for (let n = index + 1; n > 0; n = Math.floor((n - 1) / 26))
    name = String.fromCharCode(65 + ((n - 1) % 26)) + name;
  return name;
}

export function cellAddress(row: number, col: number): string {
  return `${columnName(col)}${row + 1}`;
}

/** `B12` → zero-based row 11, column 1. */
export function parseAddress(
  address: string,
): { row: number; col: number } | null {
  const match = /^\$?([A-Z]+)\$?(\d+)$/i.exec(address.trim());
  if (!match) return null;
  let col = 0;
  for (const char of match[1].toUpperCase())
    col = col * 26 + char.charCodeAt(0) - 64;
  return { row: Number(match[2]) - 1, col: col - 1 };
}

/** Excel's character widths and point heights, in screen pixels. */
export function columnPixels(width: number): number {
  return Math.round(width * 7 + 5);
}

export function rowPixels(points: number): number {
  return Math.round((points * 4) / 3);
}

// ---------------------------------------------------------------- colours

/** Office's default palette, in the order a theme index counts them. */
const OFFICE_THEME = [
  "#ffffff", // lt1
  "#000000", // dk1
  "#e7e6e6", // lt2
  "#44546a", // dk2
  "#4472c4",
  "#ed7d31",
  "#a5a5a5",
  "#ffc000",
  "#5b9bd5",
  "#70ad47",
  "#0563c1",
  "#954f72",
];

/**
 * The workbook's theme colours by theme index. The XML lists dark before
 * light (dk1, lt1, dk2, lt2…) while an index counts light first, so the first
 * two pairs swap.
 */
export function themeColors(xml: string | undefined): string[] {
  if (!xml) return OFFICE_THEME;
  const names = [
    "dk1",
    "lt1",
    "dk2",
    "lt2",
    "accent1",
    "accent2",
    "accent3",
    "accent4",
    "accent5",
    "accent6",
    "hlink",
    "folHlink",
  ];
  const found = names.map((name) => {
    const block =
      new RegExp(`<a:${name}>([\\s\\S]*?)</a:${name}>`).exec(xml)?.[1] ?? "";
    const hex =
      /lastClr="([0-9a-f]{6})"/i.exec(block)?.[1] ??
      /srgbClr val="([0-9a-f]{6})"/i.exec(block)?.[1];
    return hex ? `#${hex.toLowerCase()}` : undefined;
  });
  const [dk1, lt1, dk2, lt2, ...rest] = found;
  return [lt1, dk1, lt2, dk2, ...rest].map(
    (color, index) => color ?? OFFICE_THEME[index],
  );
}

function toRgb(hex: string): [number, number, number] {
  const value = parseInt(hex.slice(1), 16);
  return [(value >> 16) & 255, (value >> 8) & 255, value & 255];
}

function toHex([r, g, b]: number[]): string {
  return `#${[r, g, b]
    .map((part) =>
      Math.round(Math.min(255, Math.max(0, part)))
        .toString(16)
        .padStart(2, "0"),
    )
    .join("")}`;
}

/** Excel's tint: toward white when positive, toward black when negative. */
export function applyTint(hex: string, tint: number): string {
  if (!tint) return hex;
  return toHex(
    toRgb(hex).map((part) =>
      tint < 0 ? part * (1 + tint) : part + (255 - part) * tint,
    ),
  );
}

type ExcelColor =
  | { argb?: string; theme?: number; tint?: number; indexed?: number }
  | undefined;

/** The legacy palette's first entries, the only ones files still use often. */
const INDEXED = [
  "#000000",
  "#ffffff",
  "#ff0000",
  "#00ff00",
  "#0000ff",
  "#ffff00",
  "#ff00ff",
  "#00ffff",
];

export function excelColor(
  color: ExcelColor,
  theme: readonly string[],
): string | undefined {
  if (!color) return undefined;
  if (color.argb && /^[0-9a-f]{8}$/i.test(color.argb)) {
    // A fully transparent colour is no colour.
    if (color.argb.slice(0, 2) === "00" && color.argb !== "00000000")
      return undefined;
    return applyTint(`#${color.argb.slice(2).toLowerCase()}`, color.tint ?? 0);
  }
  if (typeof color.theme === "number" && theme[color.theme])
    return applyTint(theme[color.theme], color.tint ?? 0);
  if (typeof color.indexed === "number") {
    const index =
      color.indexed >= 8 && color.indexed < 16
        ? color.indexed - 8
        : color.indexed;
    return INDEXED[index];
  }
  return undefined;
}

/** Black or white, whichever reads on the fill. */
export function readableOn(fill: string): string {
  const [r, g, b] = toRgb(fill);
  return 0.299 * r + 0.587 * g + 0.114 * b > 150 ? "#000000" : "#ffffff";
}

// ---------------------------------------------------------------- numbers and dates

/** The section of a format that applies, and whether it already shows the sign. */
function formatSection(
  format: string,
  value: number,
): { section: string; signed: boolean } {
  const sections = splitSections(format);
  if (value < 0 && sections[1] !== undefined)
    return { section: sections[1], signed: true };
  if (value === 0 && sections[2] !== undefined)
    return { section: sections[2], signed: false };
  return { section: sections[0], signed: false };
}

function splitSections(format: string): string[] {
  const sections: string[] = [];
  let current = "";
  let quoted = false;
  for (let index = 0; index < format.length; index++) {
    const char = format[index];
    if (char === '"') quoted = !quoted;
    if (char === "\\" && !quoted) {
      current += char + (format[index + 1] ?? "");
      index++;
      continue;
    }
    if (char === ";" && !quoted) {
      sections.push(current);
      current = "";
    } else current += char;
  }
  sections.push(current);
  return sections;
}

/** The literal text around a number's digits: quoted strings, escapes, currency. */
function literals(section: string): {
  before: string;
  after: string;
  body: string;
} {
  let before = "";
  let after = "";
  let body = "";
  let seenDigits = false;
  for (let index = 0; index < section.length; index++) {
    const char = section[index];
    let literal = "";
    if (char === '"') {
      const end = section.indexOf('"', index + 1);
      literal = section.slice(index + 1, end < 0 ? undefined : end);
      index = end < 0 ? section.length : end;
    } else if (char === "\\") {
      literal = section[index + 1] ?? "";
      index++;
    } else if (char === "[") {
      const end = section.indexOf("]", index);
      const inside = section.slice(index + 1, end < 0 ? undefined : end);
      // `[$S/-280A]` carries a currency symbol; colours and conditions don't print.
      if (inside.startsWith("$")) literal = inside.slice(1).split("-")[0];
      index = end < 0 ? section.length : end;
    } else if (char === "_" || char === "*") {
      // Padding to the width of the next character.
      index++;
      literal = char === "_" ? " " : "";
    } else if ("0#?.,%".includes(char)) {
      seenDigits = true;
      body += char;
      continue;
    } else if (char === "E" || char === "e") {
      body += char;
      continue;
    } else literal = char === "@" ? "" : char;
    if (seenDigits) after += literal;
    else before += literal;
  }
  return { before, after, body };
}

const DATE_TOKEN =
  /yyyy|yy|mmmmm|mmmm|mmm|mm|m|dddd|ddd|dd|d|hh|h|ss|s|am\/pm|a\/p/gi;

export function isDateFormat(format: string | undefined): boolean {
  if (!format) return false;
  const plain = format.replace(/"[^"]*"|\\.|\[[^\]]*\]/g, "");
  return (
    /[ymdhs]/i.test(plain) && !/[0#?]/.test(plain.replace(/[ymdhs]/gi, ""))
  );
}

/**
 * A number as its format prints it. Covers what files use: decimals,
 * thousands, percent, currency and literal units, negative sections; anything
 * stranger falls back to the plain number.
 */
export function formatNumber(
  value: number,
  format?: string,
  locale?: string,
): string {
  if (!Number.isFinite(value)) return String(value);
  if (!format || /^general$/i.test(format.trim()))
    return generalNumber(value, locale);
  const { section, signed } = formatSection(format, value);
  const { before, after, body } = literals(section);
  // A section of text alone, like `"-"` for zero, prints just that text.
  if (!body && (before || after)) return before + after;
  if (!body || /e/i.test(body)) return generalNumber(value, locale);
  const percent = body.includes("%") ? 100 : 1;
  const [integerPart, fraction = ""] = body.replace(/%/g, "").split(".");
  const decimals = (fraction.match(/[0#?]/g) ?? []).length;
  const minimum = (fraction.match(/0/g) ?? []).length;
  const grouping = integerPart.includes(",") && /[0#?],[0#?]/.test(integerPart);
  // Trailing commas scale by thousands.
  const scale = 1000 ** (integerPart.match(/,+$/)?.[0].length ?? 0);
  const scaled = ((signed ? Math.abs(value) : value) * percent) / scale;
  const digits = new Intl.NumberFormat(locale, {
    minimumFractionDigits: minimum,
    maximumFractionDigits: decimals,
    useGrouping: grouping,
  }).format(scaled);
  return `${before}${digits}${body.includes("%") ? "%" : ""}${after}`;
}

function generalNumber(value: number, locale?: string): string {
  if (value !== 0 && (Math.abs(value) >= 1e11 || Math.abs(value) < 1e-9))
    return value
      .toExponential(5)
      .replace(/\.?0+e/, "E")
      .replace("e", "E");
  return new Intl.NumberFormat(locale, {
    maximumFractionDigits: 10,
    useGrouping: false,
  }).format(Number(value.toPrecision(11)));
}

/**
 * A date as its format prints it. Excel dates have no time zone; ExcelJS hands
 * them over as UTC, so they are read in UTC.
 */
export function formatDate(
  date: Date,
  format?: string,
  locale?: string,
): string {
  if (Number.isNaN(date.getTime())) return "";
  const section = format ? splitSections(format)[0] : "";
  if (!section || !isDateFormat(section)) {
    const hasTime =
      date.getUTCHours() || date.getUTCMinutes() || date.getUTCSeconds();
    return new Intl.DateTimeFormat(locale, {
      timeZone: "UTC",
      dateStyle: "short",
      ...(hasTime ? { timeStyle: "short" as const } : {}),
    }).format(date);
  }
  const twelve = /am\/pm|a\/p/i.test(section);
  const hours = date.getUTCHours();
  const month = (style: "long" | "short" | "narrow") =>
    new Intl.DateTimeFormat(locale, { timeZone: "UTC", month: style }).format(
      date,
    );
  const weekday = (style: "long" | "short") =>
    new Intl.DateTimeFormat(locale, { timeZone: "UTC", weekday: style }).format(
      date,
    );
  const pad = (value: number) => String(value).padStart(2, "0");
  const tokens = [
    ...section
      .replace(/\[[^\]]*\]/g, "")
      .matchAll(new RegExp(`"[^"]*"|\\\\.|${DATE_TOKEN.source}|.`, "gi")),
  ].map((match) => match[0]);
  return tokens
    .map((token, index) => {
      const lower = token.toLowerCase();
      if (token.startsWith('"')) return token.slice(1, -1);
      if (token.startsWith("\\")) return token.slice(1);
      // `m` after an hour or before seconds is minutes.
      const minutes =
        (lower === "m" || lower === "mm") &&
        (tokens
          .slice(0, index)
          .reverse()
          .find((t) => /^[a-z]/i.test(t))
          ?.toLowerCase()
          .startsWith("h") ||
          tokens
            .slice(index + 1)
            .find((t) => /^[a-z]/i.test(t))
            ?.toLowerCase()
            .startsWith("s"));
      if (minutes)
        return lower === "mm"
          ? pad(date.getUTCMinutes())
          : String(date.getUTCMinutes());
      switch (lower) {
        case "yyyy":
          return String(date.getUTCFullYear());
        case "yy":
          return pad(date.getUTCFullYear() % 100);
        case "mmmmm":
          return month("narrow");
        case "mmmm":
          return month("long");
        case "mmm":
          return month("short");
        case "mm":
          return pad(date.getUTCMonth() + 1);
        case "m":
          return String(date.getUTCMonth() + 1);
        case "dddd":
          return weekday("long");
        case "ddd":
          return weekday("short");
        case "dd":
          return pad(date.getUTCDate());
        case "d":
          return String(date.getUTCDate());
        case "hh":
          return pad(twelve ? hours % 12 || 12 : hours);
        case "h":
          return String(twelve ? hours % 12 || 12 : hours);
        case "ss":
          return pad(date.getUTCSeconds());
        case "s":
          return String(date.getUTCSeconds());
        case "am/pm":
          return hours < 12 ? "AM" : "PM";
        case "a/p":
          return hours < 12 ? "A" : "P";
        default:
          return token;
      }
    })
    .join("");
}

/** The ISO form the formula bar shows for a date, as Excel shows its own. */
function dateInput(date: Date): string {
  if (Number.isNaN(date.getTime())) return "";
  const day = date.toISOString().slice(0, 10);
  const time = date.toISOString().slice(11, 19);
  return time === "00:00:00" ? day : `${day} ${time}`;
}

// ---------------------------------------------------------------- CSV

/** RFC 4180 rows: quoted fields may hold the delimiter, quotes and line breaks. */
export function parseDelimited(text: string, delimiter: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  for (let index = 0; index < text.length; index++) {
    const char = text[index];
    if (quoted) {
      if (char === '"') {
        if (text[index + 1] === '"') {
          field += '"';
          index++;
        } else quoted = false;
      } else field += char;
      continue;
    }
    if (char === '"' && field === "") quoted = true;
    else if (char === delimiter) {
      row.push(field);
      field = "";
    } else if (char === "\n" || char === "\r") {
      if (char === "\r" && text[index + 1] === "\n") index++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else field += char;
  }
  if (field !== "" || row.length) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

/** Comma, unless the first line is clearly split by semicolons or tabs. */
export function sniffDelimiter(text: string, path: string): string {
  if (extensionOf(path) === ".tsv") return "\t";
  const end = text.search(/\r?\n/);
  const line = end < 0 ? text : text.slice(0, end);
  const count = (char: string) => line.split(char).length - 1;
  const candidates = [",", ";", "\t"].sort((a, b) => count(b) - count(a));
  return count(candidates[0]) > 0 ? candidates[0] : ",";
}

const NUMBER_TEXT = /^[-+]?(\d+([.,]\d+)?|\d{1,3}([ ,.]\d{3})+([.,]\d+)?)%?$/;

export function csvWorkbook(text: string, path: string): Workbook {
  const clean = text.replace(/^﻿/, "");
  const rows = parseDelimited(clean, sniffDelimiter(clean, path));
  const cells: SheetCell[] = [];
  let colCount = 0;
  let truncated = false;
  let rowCount = 0;
  for (const [row, fields] of rows.entries()) {
    if (cells.length + fields.length > MAX_CELLS) {
      truncated = true;
      break;
    }
    rowCount = row + 1;
    colCount = Math.max(colCount, fields.length);
    for (const [col, value] of fields.entries())
      if (value !== "")
        cells.push({
          row,
          col,
          text: value,
          input: value,
          numeric: NUMBER_TEXT.test(value.trim()) || undefined,
        });
  }
  return {
    sheets: [
      layout(
        basename(path).replace(/\.[^.]+$/, "") || "Sheet1",
        rowCount,
        colCount,
        cells,
        [],
        [],
        0,
        0,
        true,
        truncated,
      ),
    ],
  };
}

function layout(
  name: string,
  dataRows: number,
  dataCols: number,
  cells: SheetCell[],
  colWidths: number[],
  rowHeights: number[],
  frozenRows: number,
  frozenCols: number,
  gridlines: boolean,
  truncated: boolean,
  defaultCol = columnPixels(DEFAULT_COL_WIDTH),
  defaultRow = rowPixels(DEFAULT_ROW_HEIGHT),
): Sheet {
  const rowCount = Math.max(MIN_ROWS, dataRows + (truncated ? 0 : PAD_ROWS));
  const colCount = Math.max(MIN_COLS, dataCols + PAD_COLS);
  return {
    name,
    rowCount,
    colCount,
    colWidths: Array.from(
      { length: colCount },
      (_, index) => colWidths[index] ?? defaultCol,
    ),
    rowHeights: Array.from(
      { length: rowCount },
      (_, index) => rowHeights[index] ?? defaultRow,
    ),
    cells,
    frozenRows: Math.min(frozenRows, rowCount),
    frozenCols: Math.min(frozenCols, colCount),
    gridlines,
    truncated,
  };
}

// ---------------------------------------------------------------- xlsx

/* ExcelJS's shapes, as much of them as this reads. */
type XColor = ExcelColor;
type XBorderSide = { style?: string; color?: XColor };
type XStyle = {
  numFmt?: string;
  font?: {
    bold?: boolean;
    italic?: boolean;
    underline?: boolean | string;
    strike?: boolean;
    size?: number;
    name?: string;
    color?: XColor;
  };
  fill?: {
    type?: string;
    pattern?: string;
    fgColor?: XColor;
    bgColor?: XColor;
    stops?: { color?: XColor }[];
  };
  alignment?: {
    horizontal?: string;
    vertical?: string;
    wrapText?: boolean;
    indent?: number;
  };
  border?: {
    top?: XBorderSide;
    right?: XBorderSide;
    bottom?: XBorderSide;
    left?: XBorderSide;
  };
};
type XCell = {
  type: number;
  value: unknown;
  formula?: string;
  result?: unknown;
  numFmt?: string;
  style: XStyle;
};
type XRow = {
  height?: number;
  hidden?: boolean;
  cellCount: number;
  getCell(col: number): XCell;
};
type XWorksheet = {
  name: string;
  state?: string;
  rowCount: number;
  columnCount: number;
  properties?: { defaultRowHeight?: number; defaultColWidth?: number };
  views?: {
    state?: string;
    xSplit?: number;
    ySplit?: number;
    showGridLines?: boolean;
  }[];
  getColumn(col: number): { width?: number; hidden?: boolean };
  findRow(row: number): XRow | undefined;
  _merges?: Record<
    string,
    { top: number; left: number; bottom: number; right: number }
  >;
};
type XWorkbook = {
  worksheets: XWorksheet[];
  _themes?: Record<string, string>;
  xlsx: { load(data: ArrayBuffer | Uint8Array): Promise<unknown> };
};

/** The two of ExcelJS's `ValueType`s read by number. */
const ValueType = { Merge: 1, Formula: 6 } as const;

function border(
  side: XBorderSide | undefined,
  theme: readonly string[],
): CellBorder | undefined {
  if (!side?.style) return undefined;
  const width = /medium|thick/.test(side.style)
    ? side.style === "thick"
      ? 3
      : 2
    : 1;
  const style =
    side.style === "double"
      ? "double"
      : /dash/i.test(side.style)
        ? "dashed"
        : /dot|hair/i.test(side.style)
          ? "dotted"
          : "solid";
  return {
    width: style === "double" ? 3 : width,
    style,
    color: automatic(side.color) ? undefined : excelColor(side.color, theme),
  };
}

/** Black or white set on purpose reads as Excel's "automatic". */
function automatic(color: XColor): boolean {
  if (!color) return true;
  if (color.theme === 0 || color.theme === 1) return !color.tint;
  if (color.argb) return /^..(000000|ffffff)$/i.test(color.argb);
  return color.indexed === 64 || color.indexed === 8 || color.indexed === 9;
}

export function cellStyle(
  style: XStyle | undefined,
  theme: readonly string[],
): CellStyle | undefined {
  if (!style) return undefined;
  const out: CellStyle = {};
  const fill = style.fill;
  if (fill?.type === "pattern" && fill.pattern && fill.pattern !== "none")
    out.fill =
      excelColor(fill.fgColor, theme) ?? excelColor(fill.bgColor, theme);
  else if (fill?.type === "gradient")
    out.fill = excelColor(fill.stops?.[0]?.color, theme);
  const font = style.font;
  if (font) {
    if (font.bold) out.bold = true;
    if (font.italic) out.italic = true;
    if (font.underline) out.underline = true;
    if (font.strike) out.strike = true;
    if (font.size && font.size !== 11) out.size = font.size;
    if (font.name && font.name !== "Calibri") out.font = font.name;
    if (!automatic(font.color)) out.color = excelColor(font.color, theme);
  }
  const alignment = style.alignment;
  if (alignment) {
    const horizontal = alignment.horizontal;
    if (horizontal === "center" || horizontal === "centerContinuous")
      out.align = "center";
    else if (horizontal === "right") out.align = "right";
    else if (horizontal === "left") out.align = "left";
    else if (horizontal === "justify" || horizontal === "distributed")
      out.align = "justify";
    if (alignment.vertical === "top") out.valign = "top";
    else if (alignment.vertical === "middle" || alignment.vertical === "center")
      out.valign = "middle";
    if (alignment.wrapText) out.wrap = true;
    if (alignment.indent) out.indent = alignment.indent;
  }
  for (const side of ["top", "right", "bottom", "left"] as const) {
    const value = border(style.border?.[side], theme);
    if (value) out[side] = value;
  }
  for (const key of Object.keys(out) as (keyof CellStyle)[])
    if (out[key] === undefined) delete out[key];
  return Object.keys(out).length ? out : undefined;
}

function richText(value: unknown): string {
  const runs = (value as { richText?: { text?: string }[] })?.richText;
  return runs ? runs.map((run) => run.text ?? "").join("") : "";
}

/** A value and how it prints, given the cell's number format. */
function printed(
  value: unknown,
  format: string | undefined,
  locale?: string,
): { text: string; numeric?: boolean; error?: boolean } {
  if (value === null || value === undefined) return { text: "" };
  if (typeof value === "number")
    return { text: formatNumber(value, format, locale), numeric: true };
  if (typeof value === "boolean") return { text: value ? "TRUE" : "FALSE" };
  if (value instanceof Date)
    return { text: formatDate(value, format, locale), numeric: true };
  if (typeof value === "string") return { text: value };
  if (typeof value === "object") {
    if ("error" in value)
      return { text: String((value as { error: unknown }).error), error: true };
    if ("richText" in value) return { text: richText(value) };
    if ("text" in value)
      return { text: String((value as { text: unknown }).text ?? "") };
  }
  return { text: String(value) };
}

function input(value: unknown): string {
  if (value === null || value === undefined) return "";
  if (value instanceof Date) return dateInput(value);
  if (typeof value === "boolean") return value ? "TRUE" : "FALSE";
  if (typeof value === "number") return String(value);
  return printed(value, undefined).text;
}

export function readCell(
  cell: XCell,
  row: number,
  col: number,
  theme: readonly string[],
  locale?: string,
): SheetCell | null {
  if (cell.type === ValueType.Merge) return null;
  const style = cellStyle(cell.style, theme);
  const format = cell.numFmt ?? cell.style?.numFmt;
  let out: SheetCell;
  if (cell.type === ValueType.Formula) {
    const formula = `=${cell.formula ?? ""}`;
    const result = cell.result;
    if (result === undefined || result === null || result === "")
      out = {
        row,
        col,
        text: result === "" ? "" : formula,
        input: formula,
        uncalculated: result === "" ? undefined : true,
      };
    else {
      out = { row, col, ...printed(result, format, locale), input: formula };
      if (typeof result === "number") out.value = result;
    }
  } else {
    out = {
      row,
      col,
      ...printed(cell.value, format, locale),
      input: input(cell.value),
    };
    if (typeof cell.value === "number") out.value = cell.value;
  }
  if (style) out.style = style;
  if (!out.numeric) delete out.numeric;
  if (!out.error) delete out.error;
  if (!out.uncalculated) delete out.uncalculated;
  return out.text || out.style ? out : null;
}

function readSheet(
  sheet: XWorksheet,
  theme: readonly string[],
  locale?: string,
): Sheet {
  const cells: SheetCell[] = [];
  const rowHeights: number[] = [];
  const colWidths: number[] = [];
  const defaultRow = rowPixels(
    sheet.properties?.defaultRowHeight || DEFAULT_ROW_HEIGHT,
  );
  const defaultCol = columnPixels(
    sheet.properties?.defaultColWidth || DEFAULT_COL_WIDTH,
  );
  let truncated = false;
  let dataRows = 0;
  let dataCols = 0;
  const lastRow = sheet.rowCount;
  for (let r = 1; r <= lastRow; r++) {
    const row = sheet.findRow(r);
    if (!row) continue;
    rowHeights[r - 1] = row.hidden
      ? 0
      : row.height
        ? rowPixels(row.height)
        : defaultRow;
    if (cells.length + row.cellCount > MAX_CELLS) {
      truncated = true;
      break;
    }
    for (let c = 1; c <= row.cellCount; c++) {
      const cell = readCell(row.getCell(c), r - 1, c - 1, theme, locale);
      if (!cell) continue;
      cells.push(cell);
      dataRows = Math.max(dataRows, r);
      dataCols = Math.max(dataCols, c);
    }
  }
  const byPosition = new Map(
    cells.map((cell) => [cell.row * 16384 + cell.col, cell]),
  );
  for (const merge of Object.values(sheet._merges ?? {})) {
    if (truncated && merge.top > dataRows) continue;
    const key = (merge.top - 1) * 16384 + (merge.left - 1);
    let master = byPosition.get(key);
    if (!master) {
      master = { row: merge.top - 1, col: merge.left - 1, text: "", input: "" };
      cells.push(master);
      byPosition.set(key, master);
    }
    master.rowSpan = merge.bottom - merge.top + 1;
    master.colSpan = merge.right - merge.left + 1;
    dataRows = Math.max(dataRows, merge.bottom);
    dataCols = Math.max(dataCols, merge.right);
  }
  const lastCol = Math.max(dataCols, sheet.columnCount) + PAD_COLS;
  for (let c = 1; c <= lastCol; c++) {
    const column = sheet.getColumn(c);
    colWidths[c - 1] = column.hidden
      ? 0
      : column.width
        ? columnPixels(column.width)
        : defaultCol;
  }
  const view = sheet.views?.[0];
  const frozen = view?.state === "frozen";
  return layout(
    sheet.name,
    dataRows,
    dataCols,
    cells,
    colWidths,
    rowHeights,
    frozen ? (view?.ySplit ?? 0) : 0,
    frozen ? (view?.xSplit ?? 0) : 0,
    view?.showGridLines !== false,
    truncated,
    defaultCol,
    defaultRow,
  );
}

async function loadExcel(): Promise<new () => XWorkbook> {
  const module = (await import("exceljs")) as unknown as {
    default?: { Workbook: new () => XWorkbook };
    Workbook?: new () => XWorkbook;
  };
  const Workbook = module.Workbook ?? module.default?.Workbook;
  if (!Workbook) throw new Error("The spreadsheet reader did not load.");
  return Workbook;
}

/** Read a spreadsheet file's bytes into the sheets the viewer draws. */
export async function parseWorkbook(
  bytes: Uint8Array,
  path: string,
  locale?: string,
): Promise<Workbook> {
  const extension = extensionOf(path);
  if (extension === ".csv" || extension === ".tsv")
    return csvWorkbook(new TextDecoder().decode(bytes), path);
  if (extension === ".xls")
    throw new Error(
      "This is the old binary Excel format (.xls). Open it in Excel, or save it as .xlsx to read it here.",
    );
  const Workbook = await loadExcel();
  const workbook = new Workbook();
  try {
    await workbook.xlsx.load(bytes);
  } catch (cause) {
    throw new Error(
      `This file could not be read as a workbook${cause instanceof Error && cause.message ? `: ${cause.message}` : "."}`,
    );
  }
  const theme = themeColors(workbook._themes?.theme1);
  // Hidden sheets are read too: a chart may draw from one.
  const read = workbook.worksheets.map((sheet) => ({
    sheet: readSheet(sheet, theme, locale),
    hidden: sheet.state === "hidden" || sheet.state === "veryHidden",
  }));
  const sheets = read
    .filter((entry) => !entry.hidden)
    .map((entry) => entry.sheet);
  if (!sheets.length) throw new Error("This workbook has no visible sheets.");
  try {
    await readDrawings(
      await zipPackage(bytes),
      new Map(read.map(({ sheet }) => [sheet.name, sheet])),
      theme,
    );
  } catch {
    // Without its pictures and charts the sheet still reads.
  }
  return { sheets };
}
