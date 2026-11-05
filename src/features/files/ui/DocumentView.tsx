import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AlertCircle, ExternalLink, Folder } from "../../../shared/ui/icons";
import {
  basename,
  openPathWithDefaultApp,
  readBinaryFile,
  REMOTE_PATH_PREFIX,
  revealPath,
} from "../../../platform/tauri/fs";
import { watchFile } from "../model/fileWatch";
import { documentKind, type DocumentKind } from "../model/document";
import {
  readPptx,
  type Deck,
  type Paragraph,
  type SlideItem,
  type TextBody,
} from "../model/pptx";
import { loadPdfjs } from "../../papers/model/pdfText";
import { PaperPage } from "../../papers/ui/PaperPage";
import { FileCard } from "./BinaryFileView";

type Props = { path: string; cwd: string };

/** The page gutter around a document in the panel. */
const GUTTER = 12;
const EMU_PER_PT = 12700;
const NO_PASSAGES: readonly string[] = [];

/**
 * A PDF, Word or PowerPoint file as it reads, read-only, in the agents'
 * panel: PDF pages with their text selectable, Word pages, and slides. An
 * agent that rewrites the file shows its new version; the system's app is a
 * click away for anything this does not draw.
 */
export function DocumentView(props: Props) {
  // A new file starts over: its own pages, its own loading state.
  return <DocumentFile key={props.path} {...props} />;
}

type Loaded =
  | { status: "loading" }
  | { status: "ready"; bytes: Uint8Array; version: number }
  | { status: "error"; message: string };

function DocumentFile({ path, cwd }: Props) {
  const kind = documentKind(path);
  const [state, setState] = useState<Loaded>({ status: "loading" });
  const [reloadKey, setReloadKey] = useState(0);
  const [count, setCount] = useState<number>();

  useEffect(() => {
    let live = true;
    if (reloadKey === 0) setState({ status: "loading" });
    readBinaryFile(path).then(
      (bytes) => {
        if (live) setState({ status: "ready", bytes, version: reloadKey });
      },
      (cause: unknown) => {
        if (live)
          setState({
            status: "error",
            message: cause instanceof Error ? cause.message : String(cause),
          });
      },
    );
    return () => {
      live = false;
    };
  }, [path, reloadKey]);

  const reload = useCallback(() => setReloadKey((value) => value + 1), []);
  const fail = useCallback(
    (message: string) => setState({ status: "error", message }),
    [],
  );

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

  if (!kind || state.status === "error")
    return (
      <FileCard
        path={path}
        cwd={cwd}
        title={`Couldn’t show ${basename(path)}`}
        detail={
          state.status === "error"
            ? state.message
            : "This kind of file opens in its own app."
        }
        icon={<AlertCircle className="mx-auto mb-3 size-5 text-content/50" />}
        onRetry={kind ? reload : undefined}
        offerOpen
      />
    );

  return (
    <div className="flex h-full min-h-0 flex-col" data-document-view={kind}>
      <DocumentBar path={path} kind={kind} count={count} />
      {state.status === "loading" ? (
        <div className="grid flex-1 place-items-center text-[12px] text-content/45">
          Opening {basename(path)}…
        </div>
      ) : kind === "pdf" ? (
        <PdfDocument
          key={state.version}
          bytes={state.bytes}
          onCount={setCount}
          onError={fail}
        />
      ) : kind === "docx" ? (
        <WordDocument
          key={state.version}
          bytes={state.bytes}
          onCount={setCount}
          onError={fail}
        />
      ) : (
        <SlideDeck
          key={state.version}
          bytes={state.bytes}
          onCount={setCount}
          onError={fail}
        />
      )}
    </div>
  );
}

const COUNT_NOUN: Record<DocumentKind, [string, string]> = {
  pdf: ["page", "pages"],
  docx: ["page", "pages"],
  pptx: ["slide", "slides"],
};

function DocumentBar({
  path,
  kind,
  count,
}: {
  path: string;
  kind: DocumentKind;
  count?: number;
}) {
  const local = !path.startsWith(REMOTE_PATH_PREFIX);
  const [one, many] = COUNT_NOUN[kind];
  return (
    <div className="flex h-8 shrink-0 items-center gap-1 border-b border-stroke px-3 text-[12px] text-content/50">
      <span className="min-w-0 flex-1 truncate tabular-nums">
        {count != null ? `${count} ${count === 1 ? one : many}` : ""}
      </span>
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

/** The width the panel gives a page, kept as the panel resizes. */
function usePageWidth() {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => setWidth(Math.max(0, el.clientWidth - GUTTER * 2));
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, []);
  return { ref, width };
}

type ViewerProps = {
  bytes: Uint8Array;
  onCount: (count: number) => void;
  onError: (message: string) => void;
};

const message = (cause: unknown) =>
  cause instanceof Error ? cause.message : String(cause);

function PdfDocument({ bytes, onCount, onError }: ViewerProps) {
  const { ref, width } = usePageWidth();
  const [doc, setDoc] = useState<PDFDocumentProxy>();
  const [ratio, setRatio] = useState(1.414);

  useEffect(() => {
    let live = true;
    let opened: PDFDocumentProxy | undefined;
    void (async () => {
      const pdfjs = await loadPdfjs();
      // pdf.js takes the buffer over, so it gets its own copy.
      opened = await pdfjs.getDocument({ data: bytes.slice() }).promise;
      const first = await opened.getPage(1);
      const size = first.getViewport({ scale: 1 });
      if (!live) return;
      setRatio(size.height / size.width);
      setDoc(opened);
      onCount(opened.numPages);
    })().catch((cause: unknown) => live && onError(message(cause)));
    return () => {
      live = false;
      void opened?.destroy();
    };
  }, [bytes, onCount, onError]);

  return (
    <div
      ref={ref}
      className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-content/5"
      style={{ padding: GUTTER }}
    >
      {doc && width
        ? Array.from({ length: doc.numPages }, (_, index) => (
            <PaperPage
              key={index}
              doc={doc}
              number={index + 1}
              width={width}
              ratio={ratio}
              passages={NO_PASSAGES}
              eager={index === 0}
            />
          ))
        : null}
    </div>
  );
}

/** Opens a link in a document in the browser, never inside the app. */
function followLink(event: ReactMouseEvent) {
  const anchor = (event.target as Element).closest?.("a[href]");
  if (!anchor) return;
  event.preventDefault();
  const href = anchor.getAttribute("href") ?? "";
  if (/^(https?:|mailto:)/i.test(href)) void openUrl(href).catch(() => {});
  else if (href.startsWith("#"))
    anchor.ownerDocument
      .getElementById(decodeURIComponent(href.slice(1)))
      ?.scrollIntoView({ block: "start", behavior: "smooth" });
}

function WordDocument({ bytes, onCount, onError }: ViewerProps) {
  const { ref, width } = usePageWidth();
  const body = useRef<HTMLDivElement>(null);
  const styles = useRef<HTMLDivElement>(null);
  const [pageWidth, setPageWidth] = useState(0);

  useEffect(() => {
    let live = true;
    const host = body.current;
    const sheet = styles.current;
    if (!host || !sheet) return;
    void (async () => {
      const { renderAsync } = await import("docx-preview");
      await renderAsync(bytes.slice(), host, sheet, {
        className: "docx",
        inWrapper: true,
        breakPages: true,
        ignoreLastRenderedPageBreak: true,
        renderHeaders: true,
        renderFooters: true,
        renderFootnotes: true,
        renderEndnotes: true,
        // Embedded HTML would run inside the app.
        renderAltChunks: false,
        renderComments: false,
        useBase64URL: true,
      });
      if (!live) return;
      const pages = host.querySelectorAll<HTMLElement>("section.docx");
      setPageWidth(pages[0]?.offsetWidth ?? 0);
      onCount(pages.length);
    })().catch((cause: unknown) => live && onError(message(cause)));
    return () => {
      live = false;
      host.replaceChildren();
      sheet.replaceChildren();
    };
  }, [bytes, onCount, onError]);

  // Pages keep the size the file gives them, scaled down to fit the panel.
  const zoom = pageWidth && width ? Math.min(1, width / pageWidth) : 1;
  return (
    <div
      ref={ref}
      className="min-h-0 flex-1 overflow-auto overscroll-contain bg-content/5 [&_.docx-wrapper]:!bg-transparent [&_.docx-wrapper]:!p-0 [&_section.docx]:!mb-3"
      style={{ padding: GUTTER }}
      onClick={followLink}
    >
      <div ref={styles} hidden />
      <div ref={body} style={{ zoom } as CSSProperties} />
    </div>
  );
}

function SlideDeck({ bytes, onCount, onError }: ViewerProps) {
  const { ref, width } = usePageWidth();
  const [deck, setDeck] = useState<Deck>();

  useEffect(() => {
    let live = true;
    readPptx(bytes).then(
      (read) => {
        if (!live) return;
        setDeck(read);
        onCount(read.slides.length);
      },
      (cause: unknown) => live && onError(message(cause)),
    );
    return () => {
      live = false;
    };
  }, [bytes, onCount, onError]);

  return (
    <div
      ref={ref}
      className="min-h-0 flex-1 overflow-y-auto overscroll-contain bg-content/5"
      style={{ padding: GUTTER }}
    >
      {deck && width
        ? deck.slides.map((slide) => {
            const scale = width / deck.width;
            return (
              <figure key={slide.number} className="mb-3" data-slide={slide.number}>
                <div
                  className="relative overflow-hidden rounded-sm shadow-[0_0_0_1px_rgb(0_0_0/0.08)]"
                  style={{
                    width,
                    height: deck.height * scale,
                    background: slide.background ?? "#ffffff",
                    ...(slide.backgroundImage
                      ? {
                          backgroundImage: `url("${slide.backgroundImage}")`,
                          backgroundSize: "cover",
                        }
                      : {}),
                  }}
                >
                  {slide.items.map((item, index) => (
                    <SlideBox key={index} item={item} scale={scale} />
                  ))}
                </div>
                <figcaption className="mt-1 text-center text-[11px] tabular-nums text-content/40">
                  {slide.number}
                </figcaption>
              </figure>
            );
          })
        : null}
    </div>
  );
}

function frameStyle(item: SlideItem, scale: number): CSSProperties {
  const transforms = [
    item.rotate ? `rotate(${item.rotate}deg)` : "",
    item.flipH ? "scaleX(-1)" : "",
    item.flipV ? "scaleY(-1)" : "",
  ].filter(Boolean);
  return {
    position: "absolute",
    left: item.x * scale,
    top: item.y * scale,
    width: item.w * scale,
    height: item.h * scale,
    ...(transforms.length ? { transform: transforms.join(" ") } : {}),
  };
}

function SlideBox({ item, scale }: { item: SlideItem; scale: number }) {
  const style = frameStyle(item, scale);
  switch (item.kind) {
    case "image":
      return (
        <img
          src={item.src}
          alt=""
          draggable={false}
          style={{ ...style, objectFit: "fill" }}
        />
      );
    case "unsupported":
      return (
        <div
          style={style}
          className="grid place-items-center border border-dashed border-black/20 bg-black/[0.03] text-[11px] text-black/45"
          title="Open the deck in PowerPoint to see this"
        >
          {item.label}
        </div>
      );
    case "table":
      return (
        <table
          style={{ ...style, height: "auto", borderCollapse: "collapse", tableLayout: "fixed" }}
        >
          <colgroup>
            {item.columns.map((column, index) => (
              <col key={index} style={{ width: column * scale }} />
            ))}
          </colgroup>
          <tbody>
            {item.rows.map((row, index) => (
              <tr key={index} style={{ height: row.height * scale }}>
                {row.cells.map((cell, cellIndex) => (
                  <td
                    key={cellIndex}
                    colSpan={cell.span}
                    style={{
                      border: "1px solid rgb(0 0 0 / 0.18)",
                      background: cell.fill,
                      verticalAlign: "top",
                      padding: 0,
                    }}
                  >
                    <SlideText body={cell.text} scale={scale} />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      );
    case "shape":
      return (
        <div
          style={{
            ...style,
            background: item.fill,
            border: item.line ? `1px solid ${item.line}` : undefined,
            borderRadius:
              item.shape === "ellipse" ? "50%" : item.shape === "round" ? "12%" : undefined,
            display: "flex",
            flexDirection: "column",
          }}
        >
          {item.text ? <SlideText body={item.text} scale={scale} /> : null}
        </div>
      );
  }
}

const JUSTIFY: Record<TextBody["anchor"], CSSProperties["justifyContent"]> = {
  top: "flex-start",
  middle: "center",
  bottom: "flex-end",
};

/** Half an inch per bullet level, as PowerPoint indents. */
const LEVEL_INDENT_EMU = 457200;

function SlideText({ body, scale }: { body: TextBody; scale: number }) {
  const [top, right, bottom, left] = body.inset.map((inset) => inset * scale);
  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        justifyContent: JUSTIFY[body.anchor],
        flex: 1,
        minHeight: "100%",
        padding: `${top}px ${right}px ${bottom}px ${left}px`,
        color: "#000000",
        fontFamily: 'Calibri, Carlito, "Segoe UI", system-ui, sans-serif',
        lineHeight: 1.15,
        overflowWrap: "anywhere",
        userSelect: "text",
      }}
    >
      {body.paragraphs.map((paragraph, index) => (
        <SlideParagraph key={index} paragraph={paragraph} scale={scale} />
      ))}
    </div>
  );
}

function SlideParagraph({
  paragraph,
  scale,
}: {
  paragraph: Paragraph;
  scale: number;
}) {
  const px = (points: number) => points * EMU_PER_PT * scale;
  const indent = paragraph.level * LEVEL_INDENT_EMU * scale;
  if (!paragraph.runs.length)
    return <div style={{ height: px(paragraph.size) * 1.15 }} />;
  return (
    <p
      style={{
        margin: 0,
        textAlign: paragraph.align,
        fontSize: px(paragraph.size),
        paddingLeft: indent + (paragraph.bullet ? px(paragraph.size) : 0),
        textIndent: paragraph.bullet ? -px(paragraph.size) : 0,
        whiteSpace: "pre-wrap",
      }}
    >
      {paragraph.bullet ? (
        <span style={{ display: "inline-block", width: px(paragraph.size), textIndent: 0 }}>
          {paragraph.bullet}
        </span>
      ) : null}
      {paragraph.runs.map((run, index) => (
        <span
          key={index}
          style={{
            fontSize: px(run.size),
            fontWeight: run.bold ? 700 : undefined,
            fontStyle: run.italic ? "italic" : undefined,
            textDecoration: run.underline ? "underline" : undefined,
            color: run.color,
          }}
        >
          {run.text}
        </span>
      ))}
    </p>
  );
}
