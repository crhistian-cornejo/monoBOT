import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { AlertCircle, ExternalLink, Folder } from "../../../shared/ui/icons";
import { copyText } from "../../../platform/tauri/clipboard";
import {
  basename,
  openPathWithDefaultApp,
  readBinaryFile,
  REMOTE_PATH_PREFIX,
  revealPath,
} from "../../../platform/tauri/fs";
import { watchFile } from "../model/fileWatch";
import {
  cellAddress,
  columnName,
  isLegacyXls,
  readableOn,
  type CellBorder,
  type Sheet,
  type SheetCell,
  type Workbook,
} from "../model/spreadsheet";
import { readSpreadsheet } from "../model/spreadsheetWorker";
import { FileCard } from "./BinaryFileView";
import { SpreadsheetChart } from "./SpreadsheetChart";
import { ImageLightbox } from "../../../shared/ui/ImageLightbox";
import { sniffImageMime } from "../model/filePreview";
import type { SheetDrawing } from "../model/spreadsheetDrawings";

const HEADER_W = 44;
const HEADER_H = 22;
/** How far a line of text may spill into empty cells to its right. */
const MAX_SPILL_COLS = 24;
const FONT =
  'Calibri, Carlito, "Segoe UI", system-ui, -apple-system, sans-serif';
/** Excel's "automatic" border: ink on the theme, readable on a fill. */
const AUTO_INK = "color-mix(in srgb, var(--color-content) 75%, transparent)";

type Props = { path: string; cwd: string; visible?: boolean };

type LoadState =
  | { status: "loading" }
  | { status: "ready"; workbook: Workbook }
  | { status: "error"; message: string };

/**
 * A read-only spreadsheet: the sheet's grid as the file draws it, a formula
 * bar for the picked cell, and a tab per sheet. Opening in Excel is a click
 * away for anything this does not show.
 */
export function SpreadsheetView(props: Props) {
  // A new file starts over: its own sheets, its own loading state.
  return <SpreadsheetFile key={props.path} {...props} />;
}

function SpreadsheetFile({ path, cwd }: Props) {
  const [state, setState] = useState<LoadState>({ status: "loading" });
  const [reloadKey, setReloadKey] = useState(0);
  const [sheetName, setSheetName] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    // A reload keeps what is shown until the new read lands.
    if (reloadKey === 0) setState({ status: "loading" });
    readBinaryFile(path)
      .then((bytes) => readSpreadsheet(bytes, path, controller.signal))
      .then(
        (workbook) => {
          if (!controller.signal.aborted)
            setState({ status: "ready", workbook });
        },
        (cause: unknown) => {
          if (controller.signal.aborted) return;
          setState({
            status: "error",
            message: cause instanceof Error ? cause.message : String(cause),
          });
        },
      );
    return () => controller.abort();
  }, [path, reloadKey]);

  const reload = useCallback(() => setReloadKey((value) => value + 1), []);

  // An agent that rewrites the file shows its new version here.
  useEffect(() => {
    let timer = 0;
    const stop = watchFile(path, () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(reload, 150);
    });
    return () => {
      window.clearTimeout(timer);
      stop();
    };
  }, [path, reload]);

  if (state.status === "loading")
    return (
      <div className="grid h-full place-items-center text-[12px] text-content/45">
        Opening {basename(path)}…
      </div>
    );

  if (state.status === "error")
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={
          isLegacyXls(path) ? basename(path) : `Couldn’t open ${basename(path)}`
        }
        detail={state.message}
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-content/50" />}
        onRetry={isLegacyXls(path) ? undefined : reload}
        offerOpen
      />
    );

  const { sheets } = state.workbook;
  const sheet = sheets.find((entry) => entry.name === sheetName) ?? sheets[0];
  return (
    <div className="flex h-full min-h-0 flex-col bg-background-base">
      <SheetGrid key={`${path}\n${sheet.name}`} sheet={sheet} path={path} />
      {sheets.length > 1 || sheet.truncated ? (
        <div
          role="tablist"
          aria-label="Sheets"
          className="flex h-8 shrink-0 items-stretch gap-0.5 overflow-x-auto border-t border-content/10 px-1.5 text-[12px]"
        >
          {sheets.map((entry) => {
            const active = entry === sheet;
            return (
              <button
                key={entry.name}
                type="button"
                role="tab"
                aria-selected={active}
                onClick={() => setSheetName(entry.name)}
                className={`my-1 shrink-0 rounded-md px-2.5 whitespace-nowrap ${
                  active
                    ? "bg-content/10 font-medium text-content"
                    : "text-content/60 hover:bg-content/6 hover:text-content"
                }`}
              >
                {entry.name}
              </button>
            );
          })}
          {sheet.truncated ? (
            <span className="ml-auto self-center pl-3 whitespace-nowrap text-[11px] text-content/45">
              Showing the first {sheet.rowCount.toLocaleString()} rows · open in
              Excel for the rest
            </span>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

/** Offsets where each column or row starts, plus where the last one ends. */
function offsets(sizes: readonly number[]): number[] {
  const out = [0];
  for (const size of sizes) out.push(out[out.length - 1] + size);
  return out;
}

/** The index whose span holds `position`, clamped to the grid. */
function indexAt(starts: readonly number[], position: number): number {
  let low = 0;
  let high = starts.length - 2;
  while (low < high) {
    const middle = (low + high + 1) >> 1;
    if (starts[middle] <= position) low = middle;
    else high = middle - 1;
  }
  return Math.max(0, low);
}

const key = (row: number, col: number) => row * 16384 + col;

type Area = { row: number; col: number; rows: number; cols: number };

function SheetGrid({ sheet, path }: { sheet: Sheet; path: string }) {
  const scroller = useRef<HTMLDivElement>(null);
  const [view, setView] = useState({ left: 0, top: 0, width: 0, height: 0 });
  const [selected, setSelected] = useState({ row: 0, col: 0 });
  // Formulas in the cells instead of their results, as Excel's Ctrl+`.
  const [showFormulas, setShowFormulas] = useState(false);
  const [enlarged, setEnlarged] = useState<{ url: string; alt: string } | null>(
    null,
  );
  const frame = useRef(0);
  const images = useDrawingUrls(sheet.drawings);

  const xs = useMemo(() => offsets(sheet.colWidths), [sheet]);
  const ys = useMemo(() => offsets(sheet.rowHeights), [sheet]);
  const { cells, merges, covered } = useMemo(() => {
    const cells = new Map<number, SheetCell>();
    const merges: SheetCell[] = [];
    const covered = new Map<number, SheetCell>();
    for (const cell of sheet.cells) {
      cells.set(key(cell.row, cell.col), cell);
      if ((cell.rowSpan ?? 1) > 1 || (cell.colSpan ?? 1) > 1) {
        merges.push(cell);
        for (let r = 0; r < (cell.rowSpan ?? 1); r++)
          for (let c = 0; c < (cell.colSpan ?? 1); c++)
            covered.set(key(cell.row + r, cell.col + c), cell);
      }
    }
    return { cells, merges, covered };
  }, [sheet]);

  const measure = useCallback(() => {
    const element = scroller.current;
    if (!element) return;
    setView({
      left: element.scrollLeft,
      top: element.scrollTop,
      width: element.clientWidth,
      height: element.clientHeight,
    });
  }, []);

  useLayoutEffect(() => {
    const element = scroller.current;
    if (!element) return;
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame.current);
    };
  }, [measure]);

  const onScroll = () => {
    cancelAnimationFrame(frame.current);
    frame.current = requestAnimationFrame(measure);
  };

  const frozenW = xs[sheet.frozenCols];
  const frozenH = ys[sheet.frozenRows];
  const bodyW = Math.max(0, view.width - HEADER_W - frozenW);
  const bodyH = Math.max(0, view.height - HEADER_H - frozenH);

  const area = (row: number, col: number): Area => {
    const master = covered.get(key(row, col));
    return master
      ? {
          row: master.row,
          col: master.col,
          rows: master.rowSpan ?? 1,
          cols: master.colSpan ?? 1,
        }
      : { row, col, rows: 1, cols: 1 };
  };
  const pick = area(selected.row, selected.col);
  const pickedCell = cells.get(key(pick.row, pick.col));

  /** Scroll so a cell outside the frozen panes is fully in view. */
  const reveal = (target: Area) => {
    const element = scroller.current;
    if (!element) return;
    if (target.col >= sheet.frozenCols) {
      const left = xs[target.col] - frozenW;
      const right = xs[target.col + target.cols] - frozenW;
      if (left < element.scrollLeft) element.scrollLeft = left;
      else if (right > element.scrollLeft + bodyW)
        element.scrollLeft = Math.min(left, right - bodyW);
    }
    if (target.row >= sheet.frozenRows) {
      const top = ys[target.row] - frozenH;
      const bottom = ys[target.row + target.rows] - frozenH;
      if (top < element.scrollTop) element.scrollTop = top;
      else if (bottom > element.scrollTop + bodyH)
        element.scrollTop = Math.min(top, bottom - bodyH);
    }
  };

  const onMouseDown = (event: ReactMouseEvent<HTMLDivElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    const x = event.clientX - box.left - HEADER_W;
    const y = event.clientY - box.top - HEADER_H;
    const col = x < 0 ? -1 : indexAt(xs, x < frozenW ? x : x + view.left);
    const row = y < 0 ? -1 : indexAt(ys, y < frozenH ? y : y + view.top);
    // A header picks the first cell of its row or column.
    if (row < 0 && col < 0) return;
    setSelected({ row: Math.max(0, row), col: Math.max(0, col) });
    scroller.current?.focus({ preventScroll: true });
  };

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if ((event.metaKey || event.ctrlKey) && event.key === "`") {
      event.preventDefault();
      setShowFormulas((value) => !value);
      return;
    }
    if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "c") {
      event.preventDefault();
      void copyText(pickedCell?.text ?? "").catch(() => {});
      return;
    }
    const step: Record<string, [number, number]> = {
      ArrowUp: [-1, 0],
      ArrowDown: [1, 0],
      ArrowLeft: [0, -1],
      ArrowRight: [0, 1],
      Enter: [event.shiftKey ? -1 : 1, 0],
      Tab: [0, event.shiftKey ? -1 : 1],
    };
    const move = step[event.key];
    if (!move) return;
    event.preventDefault();
    const [dr, dc] = move;
    // Leave a merged area from its far edge.
    let row =
      dr > 0 ? pick.row + pick.rows : dr < 0 ? pick.row - 1 : selected.row;
    let col =
      dc > 0 ? pick.col + pick.cols : dc < 0 ? pick.col - 1 : selected.col;
    row = Math.min(sheet.rowCount - 1, Math.max(0, row));
    col = Math.min(sheet.colCount - 1, Math.max(0, col));
    setSelected({ row, col });
    reveal(area(row, col));
  };

  const regionProps = {
    sheet,
    xs,
    ys,
    cells,
    merges,
    covered,
    pick,
    showFormulas,
    images,
    onEnlarge: setEnlarged,
  };
  const colsIn = (from: number, width: number): [number, number] => [
    indexAt(xs, from),
    indexAt(xs, from + width),
  ];
  const rowsIn = (from: number, height: number): [number, number] => [
    indexAt(ys, from),
    indexAt(ys, from + height),
  ];
  const scrollCols = colsIn(frozenW + view.left, bodyW);
  const scrollRows = rowsIn(frozenH + view.top, bodyH);
  const frozenCols: [number, number] = [0, sheet.frozenCols - 1];
  const frozenRows: [number, number] = [0, sheet.frozenRows - 1];
  const hasFrozenCols = sheet.frozenCols > 0;
  const hasFrozenRows = sheet.frozenRows > 0;

  return (
    <>
      <FormulaBar
        address={pick}
        cell={pickedCell}
        path={path}
        showFormulas={showFormulas}
        onToggleFormulas={() => setShowFormulas((value) => !value)}
      />
      <div
        ref={scroller}
        tabIndex={0}
        role="grid"
        aria-label={`${sheet.name} sheet`}
        aria-rowcount={sheet.rowCount}
        aria-colcount={sheet.colCount}
        onScroll={onScroll}
        onKeyDown={onKeyDown}
        className="relative min-h-0 flex-1 overflow-auto overscroll-none outline-none"
      >
        <div
          style={{
            width: HEADER_W + xs[xs.length - 1],
            height: HEADER_H + ys[ys.length - 1],
          }}
        >
          <div
            onMouseDown={onMouseDown}
            className="sticky top-0 left-0 overflow-hidden select-none"
            style={{ width: view.width, height: view.height }}
          >
            <Region
              {...regionProps}
              left={HEADER_W + frozenW}
              top={HEADER_H + frozenH}
              width={bodyW}
              height={bodyH}
              originX={frozenW + view.left}
              originY={frozenH + view.top}
              cols={scrollCols}
              rows={scrollRows}
            />
            {hasFrozenRows ? (
              <Region
                {...regionProps}
                left={HEADER_W + frozenW}
                top={HEADER_H}
                width={bodyW}
                height={frozenH}
                originX={frozenW + view.left}
                originY={0}
                cols={scrollCols}
                rows={frozenRows}
                edge="bottom"
              />
            ) : null}
            {hasFrozenCols ? (
              <Region
                {...regionProps}
                left={HEADER_W}
                top={HEADER_H + frozenH}
                width={frozenW}
                height={bodyH}
                originX={0}
                originY={frozenH + view.top}
                cols={frozenCols}
                rows={scrollRows}
                edge="right"
              />
            ) : null}
            {hasFrozenCols && hasFrozenRows ? (
              <Region
                {...regionProps}
                left={HEADER_W}
                top={HEADER_H}
                width={frozenW}
                height={frozenH}
                originX={0}
                originY={0}
                cols={frozenCols}
                rows={frozenRows}
              />
            ) : null}
            <ColumnHeaders
              xs={xs}
              left={HEADER_W + frozenW}
              width={bodyW}
              originX={frozenW + view.left}
              cols={scrollCols}
              pick={pick}
            />
            {hasFrozenCols ? (
              <ColumnHeaders
                xs={xs}
                left={HEADER_W}
                width={frozenW}
                originX={0}
                cols={frozenCols}
                pick={pick}
              />
            ) : null}
            <RowHeaders
              ys={ys}
              top={HEADER_H + frozenH}
              height={bodyH}
              originY={frozenH + view.top}
              rows={scrollRows}
              pick={pick}
            />
            {hasFrozenRows ? (
              <RowHeaders
                ys={ys}
                top={HEADER_H}
                height={frozenH}
                originY={0}
                rows={frozenRows}
                pick={pick}
              />
            ) : null}
            <div
              className="absolute top-0 left-0 border-r border-b border-content/12 bg-background-base"
              style={{ width: HEADER_W, height: HEADER_H }}
            />
          </div>
        </div>
      </div>
      {enlarged ? (
        <ImageLightbox
          src={enlarged.url}
          alt={enlarged.alt}
          onClose={() => setEnlarged(null)}
        />
      ) : null}
    </>
  );
}

/**
 * A blob URL for each picture the browser can draw, by drawing. Pictures are
 * sniffed, never trusted by name; EMF and other formats get no URL.
 */
function useDrawingUrls(
  drawings: readonly SheetDrawing[] | undefined,
): Map<SheetDrawing, string> {
  const [urls, setUrls] = useState<Map<SheetDrawing, string>>(() => new Map());
  // Made and revoked by the same effect: a remount (StrictMode, a reload)
  // gets fresh URLs instead of ones its last cleanup revoked.
  useEffect(() => {
    const out = new Map<SheetDrawing, string>();
    for (const drawing of drawings ?? []) {
      if (drawing.kind !== "image") continue;
      const mime = sniffImageMime(drawing.bytes);
      if (mime)
        out.set(
          drawing,
          URL.createObjectURL(new Blob([drawing.bytes], { type: mime })),
        );
    }
    setUrls(out);
    return () => {
      for (const url of out.values()) URL.revokeObjectURL(url);
    };
  }, [drawings]);
  return urls;
}

function FormulaBar({
  address,
  cell,
  path,
  showFormulas,
  onToggleFormulas,
}: {
  address: Area;
  cell?: SheetCell;
  path: string;
  showFormulas: boolean;
  onToggleFormulas: () => void;
}) {
  const name =
    address.rows > 1 || address.cols > 1
      ? `${cellAddress(address.row, address.col)}:${cellAddress(
          address.row + address.rows - 1,
          address.col + address.cols - 1,
        )}`
      : cellAddress(address.row, address.col);
  const local = !path.startsWith(REMOTE_PATH_PREFIX);
  return (
    <div className="flex h-8 shrink-0 items-center border-b border-content/10 text-[12px]">
      <div className="flex h-full w-20 shrink-0 items-center border-r border-content/10 px-2 font-mono text-[11px] text-content/70">
        {name}
      </div>
      <button
        type="button"
        aria-pressed={showFormulas}
        title={
          showFormulas
            ? "Show results in the cells (Ctrl+`)"
            : "Show formulas in the cells (Ctrl+`)"
        }
        onClick={onToggleFormulas}
        className={`mx-1 shrink-0 rounded px-1.5 font-serif text-[12px] italic ${
          showFormulas
            ? "bg-accent/15 text-accent"
            : "text-content/40 hover:bg-content/8 hover:text-content"
        }`}
      >
        fx
      </button>
      <div
        className="min-w-0 flex-1 truncate pr-2 text-content select-text"
        title={cell?.input || undefined}
      >
        {cell?.input ?? ""}
      </div>
      <div className="flex shrink-0 items-center gap-0.5 pr-1.5">
        {local ? (
          <BarButton
            label="Open in default app"
            onClick={() => void openPathWithDefaultApp(path).catch(() => {})}
          >
            <ExternalLink className="size-3.5" strokeWidth={1.75} />
          </BarButton>
        ) : null}
        <BarButton
          label="Reveal in folder"
          onClick={() => void revealPath(path).catch(() => {})}
        >
          <Folder className="size-3.5" strokeWidth={1.75} />
        </BarButton>
      </div>
    </div>
  );
}

function BarButton({
  label,
  onClick,
  children,
}: {
  label: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={onClick}
      className="grid size-6 place-items-center rounded-md text-content/60 hover:bg-content/8 hover:text-content"
    >
      {children}
    </button>
  );
}

function ColumnHeaders({
  xs,
  left,
  width,
  originX,
  cols,
  pick,
}: {
  xs: number[];
  left: number;
  width: number;
  originX: number;
  cols: [number, number];
  pick: Area;
}) {
  const out: ReactNode[] = [];
  for (let col = cols[0]; col <= cols[1]; col++) {
    const size = xs[col + 1] - xs[col];
    if (!size) continue;
    const on = col >= pick.col && col < pick.col + pick.cols;
    out.push(
      <div
        key={col}
        className={`absolute top-0 flex items-center justify-center border-r border-b border-content/12 text-[11px] ${
          on ? "bg-content/10 text-content" : "text-content/50"
        }`}
        style={{ left: xs[col] - originX, width: size, height: HEADER_H }}
      >
        {columnName(col)}
      </div>,
    );
  }
  return (
    <div
      className="absolute top-0 overflow-hidden bg-background-base"
      style={{ left, width, height: HEADER_H }}
    >
      {out}
    </div>
  );
}

function RowHeaders({
  ys,
  top,
  height,
  originY,
  rows,
  pick,
}: {
  ys: number[];
  top: number;
  height: number;
  originY: number;
  rows: [number, number];
  pick: Area;
}) {
  const out: ReactNode[] = [];
  for (let row = rows[0]; row <= rows[1]; row++) {
    const size = ys[row + 1] - ys[row];
    if (!size) continue;
    const on = row >= pick.row && row < pick.row + pick.rows;
    out.push(
      <div
        key={row}
        className={`absolute left-0 flex items-center justify-center border-r border-b border-content/12 text-[11px] tabular-nums ${
          on ? "bg-content/10 text-content" : "text-content/50"
        }`}
        style={{ top: ys[row] - originY, height: size, width: HEADER_W }}
      >
        {row + 1}
      </div>,
    );
  }
  return (
    <div
      className="absolute left-0 overflow-hidden bg-background-base"
      style={{ top, height, width: HEADER_W }}
    >
      {out}
    </div>
  );
}

function borderCss(
  border: CellBorder | undefined,
  ink: string,
): string | undefined {
  if (!border) return undefined;
  return `${border.width}px ${border.style} ${border.color ?? ink}`;
}

/**
 * One pane of the sheet, clipped to its box: gridlines, then fills and
 * borders, then text, which may spill over empty neighbours as in Excel, then
 * the picked cell's outline.
 */
function Region({
  sheet,
  xs,
  ys,
  cells,
  merges,
  covered,
  pick,
  left,
  top,
  width,
  height,
  originX,
  originY,
  cols,
  rows,
  edge,
  showFormulas,
  images,
  onEnlarge,
}: {
  sheet: Sheet;
  xs: number[];
  ys: number[];
  cells: Map<number, SheetCell>;
  merges: SheetCell[];
  covered: Map<number, SheetCell>;
  pick: Area;
  left: number;
  top: number;
  width: number;
  height: number;
  originX: number;
  originY: number;
  cols: [number, number];
  rows: [number, number];
  /** The frozen pane's dividing line. */
  edge?: "right" | "bottom";
  showFormulas: boolean;
  images: Map<SheetDrawing, string>;
  onEnlarge: (image: { url: string; alt: string }) => void;
}) {
  if (width <= 0 || height <= 0 || cols[1] < cols[0] || rows[1] < rows[0])
    return null;
  const lines: ReactNode[] = [];
  if (sheet.gridlines) {
    for (let col = cols[0]; col <= cols[1]; col++)
      if (xs[col + 1] > xs[col])
        lines.push(
          <div
            key={`v${col}`}
            className="absolute top-0 w-px bg-content/10"
            style={{ left: xs[col + 1] - originX - 1, height }}
          />,
        );
    for (let row = rows[0]; row <= rows[1]; row++)
      if (ys[row + 1] > ys[row])
        lines.push(
          <div
            key={`h${row}`}
            className="absolute left-0 h-px bg-content/10"
            style={{ top: ys[row + 1] - originY - 1, width }}
          />,
        );
  }

  // Merged areas that reach into view, wherever their first cell is.
  const shown: SheetCell[] = merges.filter(
    (cell) =>
      cell.row <= rows[1] &&
      cell.row + (cell.rowSpan ?? 1) - 1 >= rows[0] &&
      cell.col <= cols[1] &&
      cell.col + (cell.colSpan ?? 1) - 1 >= cols[0],
  );
  // Text spills from cells left of the view too.
  const spillFrom = Math.max(0, cols[0] - MAX_SPILL_COLS);
  for (let row = rows[0]; row <= rows[1]; row++)
    for (let col = spillFrom; col <= cols[1]; col++) {
      const cell = cells.get(key(row, col));
      if (cell && !cell.rowSpan && !cell.colSpan) {
        if (col < cols[0] && !spills(cell)) continue;
        shown.push(cell);
      }
    }

  const boxes: ReactNode[] = [];
  const texts: ReactNode[] = [];
  for (const cell of shown) {
    const x = xs[cell.col] - originX;
    const y = ys[cell.row] - originY;
    const w = xs[cell.col + (cell.colSpan ?? 1)] - xs[cell.col];
    const h = ys[cell.row + (cell.rowSpan ?? 1)] - ys[cell.row];
    if (!w || !h) continue;
    const style = cell.style;
    const fill = style?.fill;
    const ink = style?.color ?? (fill ? readableOn(fill) : undefined);
    const borderInk = fill ? readableOn(fill) : AUTO_INK;
    const id = `${cell.row}:${cell.col}`;
    if (fill || style?.top || style?.right || style?.bottom || style?.left)
      boxes.push(
        <div
          key={id}
          className="absolute"
          style={{
            left: x,
            top: y,
            width: w,
            height: h,
            background: fill,
          }}
        >
          {style?.top || style?.right || style?.bottom || style?.left ? (
            // Reaching one pixel up and left lets neighbours share a line.
            <div
              className="absolute"
              style={{
                inset: "-1px 0 0 -1px",
                borderTop: borderCss(style.top, borderInk),
                borderRight: borderCss(style.right, borderInk),
                borderBottom: borderCss(style.bottom, borderInk),
                borderLeft: borderCss(style.left, borderInk),
              }}
            />
          ) : null}
        </div>,
      );
    const formula = showFormulas && cell.input.startsWith("=");
    const shownText = formula ? cell.input : cell.text;
    if (!shownText) continue;
    const align = formula
      ? "left"
      : (style?.align ?? (cell.numeric || cell.error ? "right" : "left"));
    // Unwrapped text on the left runs on over empty cells, as Excel draws it.
    let spill = w;
    if (formula || spills(cell))
      for (
        let next = cell.col + 1;
        next < Math.min(sheet.colCount, cell.col + MAX_SPILL_COLS);
        next++
      ) {
        const neighbour = cells.get(key(cell.row, next));
        if (neighbour?.text || covered.has(key(cell.row, next))) break;
        spill += xs[next + 1] - xs[next];
      }
    const text: CSSProperties = {
      left: x,
      top: y,
      width: spill,
      height: h,
      color: ink,
      fontFamily: style?.font ? `"${style.font}", ${FONT}` : FONT,
      fontSize: `${((style?.size ?? 11) * 4) / 3}px`,
      fontWeight: style?.bold ? 700 : undefined,
      fontStyle:
        !formula && (style?.italic || cell.uncalculated) ? "italic" : undefined,
      textDecoration:
        [style?.underline && "underline", style?.strike && "line-through"]
          .filter(Boolean)
          .join(" ") || undefined,
      opacity: cell.uncalculated && !formula ? 0.55 : undefined,
      alignItems:
        style?.valign === "top"
          ? "flex-start"
          : style?.valign === "middle"
            ? "center"
            : "flex-end",
      justifyContent:
        align === "right"
          ? "flex-end"
          : align === "center"
            ? "center"
            : "flex-start",
      textAlign: align,
      whiteSpace: style?.wrap && !formula ? "pre-wrap" : "pre",
      paddingLeft: 3 + (style?.indent ?? 0) * 9,
      paddingRight: 3,
    };
    texts.push(
      <div
        key={id}
        className="absolute flex overflow-hidden leading-[1.2]"
        style={text}
        title={cell.uncalculated ? "Saved without its result" : undefined}
      >
        <span className="min-w-0 break-words">{shownText}</span>
      </div>,
    );
  }

  const pickX = xs[pick.col] - originX;
  const pickY = ys[pick.row] - originY;
  return (
    <div
      className={`absolute overflow-hidden ${
        edge === "right"
          ? "border-r border-content/25"
          : edge === "bottom"
            ? "border-b border-content/25"
            : ""
      }`}
      style={{ left, top, width, height, boxSizing: "content-box" }}
    >
      {lines}
      {boxes}
      {texts}
      {(sheet.drawings ?? []).map((drawing, index) => {
        const { x, y, width: w, height: h } = drawing.box;
        // Only what reaches into this pane.
        if (
          x - originX > width ||
          y - originY > height ||
          x + w - originX < 0 ||
          y + h - originY < 0
        )
          return null;
        const place: CSSProperties = {
          left: x - originX,
          top: y - originY,
          width: w,
          height: h,
        };
        if (drawing.kind === "chart")
          return (
            <div
              key={index}
              className="pointer-events-none absolute"
              style={place}
            >
              <SpreadsheetChart chart={drawing} />
            </div>
          );
        const url = images.get(drawing);
        const alt = drawing.description ?? drawing.name;
        return url ? (
          <img
            key={index}
            src={url}
            alt={alt}
            draggable={false}
            className="absolute cursor-zoom-in object-fill"
            style={place}
            onMouseDown={(event) => event.stopPropagation()}
            onClick={() => onEnlarge({ url, alt })}
          />
        ) : (
          <div
            key={index}
            className="absolute grid place-items-center border border-dashed border-content/25 bg-background-base p-2 text-center text-[11px] text-content/55"
            style={place}
          >
            {`${drawing.extension.toUpperCase() || "This"} picture — open the file to see it`}
          </div>
        );
      })}
      <div
        className="pointer-events-none absolute border-2 border-accent"
        style={{
          left: pickX - 1,
          top: pickY - 1,
          width: xs[pick.col + pick.cols] - xs[pick.col] + 1,
          height: ys[pick.row + pick.rows] - ys[pick.row] + 1,
        }}
      />
    </div>
  );
}

/** Whether a cell's text may run on past its right edge. */
function spills(cell: SheetCell): boolean {
  const align = cell.style?.align;
  return (
    !!cell.text &&
    !cell.style?.wrap &&
    !cell.numeric &&
    !cell.error &&
    (!align || align === "left")
  );
}
