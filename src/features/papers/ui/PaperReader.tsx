import { open as chooseFile } from "@tauri-apps/plugin-dialog";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
import { ExternalLink, Loader } from "../../../shared/ui/icons";
import { locateInItems, locatePage } from "../model/locate";
import {
  announcePaperShown,
  FOCUS_PAPER_EVENT,
  type FocusPaperRequest,
} from "../model/openPaper";
import {
  citedLabel,
  paperCitations,
  savedLocation,
  type CitedPassage,
} from "../model/paperCitations";
import { paperLabel } from "../model/papers";
import {
  importPaperPdf,
  readPaperView,
  type PaperView,
} from "../model/paperStore";
import { loadPdfjs } from "../model/pdfText";
import { focusHighlight } from "./highlight";
import { PaperPage, type PageHit } from "./PaperPage";

const STATUS: Record<PaperView["availability"], string> = {
  "full-text": "Full text",
  "abstract-only": "Abstract only",
  "metadata-only": "No text",
};

/** The abstract, with every passage an answer cited marked where it says it. */
function Abstract({
  text,
  passages,
  focus,
}: {
  text: string;
  passages: readonly string[];
  focus?: string;
}) {
  const parts = useMemo(() => {
    const spans = passages
      .flatMap((passage) => {
        const found = locateInItems([text], passage);
        return found
          ? [
              {
                from: found.startOffset,
                to: found.endOffset,
                focus: passage === focus,
              },
            ]
          : [];
      })
      .sort((a, b) => a.from - b.from);
    const merged: typeof spans = [];
    for (const span of spans) {
      const last = merged[merged.length - 1];
      if (last && span.from < last.to) {
        last.to = Math.max(last.to, span.to);
        last.focus ||= span.focus;
      } else merged.push({ ...span });
    }
    const out: { text: string; marked: boolean; focus: boolean }[] = [];
    let at = 0;
    for (const span of merged) {
      if (span.from > at)
        out.push({
          text: text.slice(at, span.from),
          marked: false,
          focus: false,
        });
      out.push({
        text: text.slice(span.from, span.to),
        marked: true,
        focus: span.focus,
      });
      at = span.to;
    }
    if (at < text.length)
      out.push({ text: text.slice(at), marked: false, focus: false });
    return out;
  }, [text, passages, focus]);
  const mark = useRef<HTMLElement>(null);
  useEffect(() => mark.current?.scrollIntoView({ block: "center" }), [parts]);
  const firstMarked = parts.findIndex((part) => part.focus);
  return (
    <p className="text-[13.5px] leading-[1.65] text-content/85">
      {parts.map((part, index) =>
        part.marked ? (
          <mark
            key={index}
            ref={
              index ===
              (firstMarked < 0 ? parts.findIndex((p) => p.marked) : firstMarked)
                ? mark
                : undefined
            }
            data-cited=""
            className="paper-cite-mark"
          >
            {part.text}
          </mark>
        ) : (
          part.text
        ),
      )}
    </p>
  );
}

/**
 * A paper the agent read: who wrote it, whether the app has its text, and the
 * PDF itself with every passage an answer cited underlined where the paper
 * says it. Without an open copy it shows the abstract, which is all there is.
 */
export function PaperReader({
  paperKey,
  passage,
  embedded = false,
  onExpand,
}: {
  paperKey: string;
  /** The sentence a click named: the viewer brings it into view. */
  passage?: string;
  /** Inside a page rather than a dialog: no close button to make room for. */
  embedded?: boolean;
  onExpand?: () => void;
}) {
  const [view, setView] = useState<PaperView | null | undefined>(undefined);
  // The passage in view: the one a click named, then whichever the chat
  // points at while the answer is read.
  const [focus, setFocus] = useState(passage);
  useEffect(() => setFocus(passage), [passage, paperKey]);
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [failed, setFailed] = useState(false);
  const [ratio, setRatio] = useState(1.3);
  const [width, setWidth] = useState(0);
  const [found, setFound] = useState<ReadonlyMap<string, boolean>>(new Map());
  const [listOpen, setListOpen] = useState(false);
  const [adding, setAdding] = useState<"idle" | "busy">("idle");
  const [addError, setAddError] = useState<string>();
  const scroller = useRef<HTMLDivElement>(null);
  const ranges = useRef(new Map<string, Range>());

  // Tell citations in the chat that this viewer shows the paper, and follow
  // them: hovering one brings its passage into view here.
  useEffect(() => announcePaperShown(paperKey), [paperKey]);
  useEffect(() => {
    const follow = (event: Event) => {
      const asked = (event as CustomEvent<FocusPaperRequest>).detail;
      if (asked.key === paperKey && asked.passage) setFocus(asked.passage);
    };
    window.addEventListener(FOCUS_PAPER_EVENT, follow);
    return () => window.removeEventListener(FOCUS_PAPER_EVENT, follow);
  }, [paperKey]);

  useEffect(() => {
    let live = true;
    setView(undefined);
    void readPaperView(paperKey).then((read) => live && setView(read ?? null));
    return () => {
      live = false;
    };
  }, [paperKey]);

  // What answers cited from this paper, plus the one a click named.
  const citedList = useMemo<CitedPassage[]>(() => {
    const list = paperCitations(paperKey);
    if (focus && !list.some((entry) => entry.passage === focus))
      list.push({ passage: focus });
    return list;
    // `view` changes when the paper is loaded, after citations were recorded.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paperKey, focus, view]);
  const cited = useMemo(
    () => citedList.map((entry) => entry.passage),
    [citedList],
  );

  useEffect(() => {
    setDoc(null);
    setFailed(false);
    setFound(new Map());
    ranges.current.clear();
    if (!view?.pdf) return;
    let live = true;
    let opened: PDFDocumentProxy | undefined;
    void (async () => {
      const pdfjs = await loadPdfjs();
      // pdf.js takes the buffer over, so it gets its own copy.
      opened = await pdfjs.getDocument({ data: view.pdf!.slice() }).promise;
      const first = await opened.getPage(1);
      const size = first.getViewport({ scale: 1 });
      if (!live) return;
      setRatio(size.height / size.width);
      setDoc(opened);
    })().catch(() => live && setFailed(true));
    return () => {
      live = false;
      void opened?.destroy();
    };
  }, [view]);

  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const measure = () =>
      setWidth(Math.max(0, Math.min(el.clientWidth - 32, 900)));
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, [view, doc]);

  // Which page holds each cited passage, from the text the paper was read into.
  const placed = useMemo(() => {
    const byPage = new Map<number, string[]>();
    const where = new Map<string, number>();
    if (!view?.text) return { byPage, where };
    for (const quoted of cited) {
      const page = locatePage(view.text.pages, quoted)?.page;
      if (!page) continue;
      where.set(quoted, page);
      byPage.set(page, [...(byPage.get(page) ?? []), quoted]);
    }
    return { byPage, where };
  }, [cited, view]);

  const scrollToRange = useCallback((range: Range) => {
    const el = scroller.current;
    if (!el) return;
    const rect = range.getBoundingClientRect();
    const box = el.getBoundingClientRect();
    el.scrollTo({
      top: el.scrollTop + rect.top - box.top - box.height / 3,
      behavior: "smooth",
    });
  }, []);

  // A passage already drawn is in `ranges`: bring it into view, and make it
  // the strong one. One not drawn yet is handled when its page reports it.
  useEffect(() => {
    const range = focus ? ranges.current.get(focus) : undefined;
    if (range) scrollToRange(range);
    focusHighlight(range);
    return () => focusHighlight(undefined);
  }, [focus, scrollToRange]);

  const onHighlight = useCallback(
    (_page: number, hits: PageHit[]) => {
      for (const hit of hits) {
        if (hit.range) ranges.current.set(hit.passage, hit.range);
        else ranges.current.delete(hit.passage);
      }
      setFound((current) => {
        const next = new Map(current);
        for (const hit of hits) next.set(hit.passage, !!hit.range);
        return next;
      });
      const focused = focus
        ? hits.find((hit) => hit.passage === focus)
        : undefined;
      if (focused?.range) {
        scrollToRange(focused.range);
        focusHighlight(focused.range);
      }
    },
    [focus, scrollToRange],
  );

  const firstCitedPage = useMemo(
    () => (placed.byPage.size ? Math.min(...placed.byPage.keys()) : undefined),
    [placed],
  );
  const focusPage = focus ? placed.where.get(focus) : undefined;
  const homePage = focusPage ?? firstCitedPage ?? 1;

  // Without a click to follow, open at the first underlined passage; a click
  // whose words are not found still brings the reader to its page.
  useEffect(() => {
    if (!doc || !width) return;
    if (focus && found.get(focus) === true) return;
    scroller.current
      ?.querySelector(`[data-paper-page="${homePage}"]`)
      ?.scrollIntoView({ block: "start" });
    // Once per document and target, not at every resize or highlight.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [doc, homePage, !!width]);

  const jumpTo = (quoted: string) => {
    const range = ranges.current.get(quoted);
    if (range) return scrollToRange(range);
    const page = placed.where.get(quoted);
    if (page)
      scroller.current
        ?.querySelector(`[data-paper-page="${page}"]`)
        ?.scrollIntoView({ block: "start", behavior: "smooth" });
    else
      scroller.current
        ?.querySelector("[data-cited]")
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
  };

  /** The user's own copy of a paper with no open one: chosen, kept, read. */
  const addPdf = async () => {
    setAddError(undefined);
    try {
      const chosen = await chooseFile({
        multiple: false,
        directory: false,
        title: "Choose the paper's PDF",
        filters: [{ name: "PDF", extensions: ["pdf"] }],
      });
      if (typeof chosen !== "string") return;
      setAdding("busy");
      await importPaperPdf(paperKey, chosen);
      setView((await readPaperView(paperKey)) ?? null);
    } catch (error) {
      setAddError(error instanceof Error ? error.message : String(error));
    } finally {
      setAdding("idle");
    }
  };

  if (view === undefined)
    return (
      <div className="grid h-full place-items-center text-content/50">
        <Loader className="size-4 animate-spin" />
      </div>
    );
  if (view === null)
    return (
      <div className="grid h-full place-items-center p-6 text-[13px] text-content/60">
        This paper is not saved on this computer yet.
      </div>
    );

  const { paper } = view;
  const link = paper.doi ? `https://doi.org/${paper.doi}` : paper.landing;
  const abstractMode = !view.pdf;
  const isFound = (quoted: string) =>
    abstractMode
      ? !!paper.abstract && !!locateInItems([paper.abstract], quoted)
      : placed.where.has(quoted)
        ? found.get(quoted)
        : view.text
          ? false
          : undefined;
  const focusMissing =
    !!focus &&
    (abstractMode
      ? isFound(focus) === false
      : found.get(focus) === false ||
        (!!doc && !focusPage && !!view.text && found.get(focus) === undefined));
  return (
    <div className="flex h-full min-h-0 flex-col">
      <header
        className={`shrink-0 border-b border-content/10 py-3 pl-5 ${embedded ? "pr-4" : "pr-12"}`}
      >
        <div className="flex items-start gap-4">
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] leading-snug font-medium text-content">
              {paper.title}
            </h2>
            <p className="mt-1 text-[12px] text-content/60">
              {[
                paperLabel(paper),
                paper.venue,
                paper.citedBy !== undefined ? `${paper.citedBy} citations` : "",
              ]
                .filter(Boolean)
                .join(" · ")}
            </p>
            {view.file ? (
              <p className="mt-1.5 flex min-w-0 items-center gap-1.5 text-[11px] text-content/50">
                <span className="min-w-0 truncate" title={view.file}>
                  Saved in {savedLocation(view.file).folder}
                </span>
                <button
                  type="button"
                  onClick={() =>
                    void revealItemInDir(view.file!).catch(() => undefined)
                  }
                  className="shrink-0 rounded px-1.5 py-0.5 text-content/60 hover:bg-content/8 hover:text-content"
                >
                  Show in folder
                </button>
              </p>
            ) : null}
          </div>
        </div>
        <div className="mt-2 flex items-center gap-2 text-[11px]">
          <span
            className={`shrink-0 rounded-full px-2 py-0.5 font-medium ${
              view.availability === "full-text"
                ? "bg-accent/15 text-accent"
                : "bg-content/8 text-content/65"
            }`}
          >
            {STATUS[view.availability]}
          </span>
          {view.availability !== "full-text" && view.reason ? (
            <span
              className="min-w-0 truncate text-content/50"
              title={view.reason}
            >
              {view.reason}
            </span>
          ) : null}
          <span className="ml-auto flex shrink-0 items-center gap-1">
            {onExpand ? (
              <button
                type="button"
                onClick={onExpand}
                className="rounded-md px-2 py-1 text-content/60 hover:bg-content/8 hover:text-content"
              >
                Open larger
              </button>
            ) : null}
            {!view.pdf ? (
              <button
                type="button"
                disabled={adding === "busy"}
                onClick={() => void addPdf()}
                className="rounded-md px-2 py-1 text-content/60 hover:bg-content/8 hover:text-content disabled:opacity-50"
              >
                {adding === "busy" ? "Reading…" : "Add PDF"}
              </button>
            ) : null}
            {link ? (
              <button
                type="button"
                onClick={() => void openUrl(link).catch(() => undefined)}
                className="flex items-center gap-1 rounded-md px-2 py-1 text-content/60 hover:bg-content/8 hover:text-content"
              >
                <ExternalLink className="size-3" /> Publisher
              </button>
            ) : null}
          </span>
        </div>
        {focus ? (
          <p className="mt-2.5 border-l-2 border-accent/60 pl-2.5 text-[12px] leading-relaxed text-content/70">
            {focus}
            {focusMissing ? (
              <span className="mt-1 block text-[11px] text-content/45">
                {focusPage
                  ? `On page ${focusPage}; the exact words did not match the PDF's text.`
                  : abstractMode
                    ? "The abstract does not hold these words."
                    : "The paper's text does not hold these words."}
              </span>
            ) : null}
          </p>
        ) : null}
        {cited.length > (focus ? 1 : 0) || (!focus && cited.length) ? (
          <div className="mt-2.5">
            <button
              type="button"
              aria-expanded={listOpen}
              onClick={() => setListOpen((open) => !open)}
              className="flex items-center gap-1.5 rounded-md py-0.5 text-[11px] text-content/55 hover:text-content"
            >
              <span aria-hidden className="size-2 rounded-full bg-accent/50" />
              {cited.length === 1
                ? "1 passage cited in answers"
                : `${cited.length} passages cited in answers`}
            </button>
            {listOpen ? (
              <ol className="mt-1.5 max-h-40 space-y-1 overflow-y-auto">
                {citedList.map(({ passage: quoted, number }) => {
                  const page = placed.where.get(quoted);
                  const missing = isFound(quoted) === false;
                  return (
                    <li key={quoted}>
                      <button
                        type="button"
                        onClick={() => jumpTo(quoted)}
                        aria-label={`${citedLabel(number, page)}${missing ? ", not found in the text" : ""}: ${quoted}`}
                        className="flex w-full items-start gap-2 rounded-md px-2 py-1 text-left hover:bg-content/6"
                      >
                        <span
                          aria-hidden
                          className="mt-px flex shrink-0 items-center gap-1.5 text-[11px] tabular-nums"
                        >
                          {/* A fixed column, so one and two digits line up. */}
                          <span className="grid w-6 place-items-center">
                            {number !== undefined ? (
                              <span className="grid h-4 min-w-4 place-items-center rounded-full bg-content/10 px-1 font-medium text-content/70">
                                {number}
                              </span>
                            ) : null}
                          </span>
                          <span className="w-7 text-content/50">
                            {page ? `p. ${page}` : "—"}
                          </span>
                        </span>
                        <span className="min-w-0 flex-1 text-[12px] leading-snug text-content/70">
                          <span className="line-clamp-2">{quoted}</span>
                          {missing ? (
                            <span className="block text-[11px] text-content/40">
                              Not found in the text
                            </span>
                          ) : null}
                        </span>
                      </button>
                    </li>
                  );
                })}
              </ol>
            ) : null}
          </div>
        ) : null}
      </header>
      <div ref={scroller} className="min-h-0 flex-1 overflow-y-auto px-4 py-4">
        {view.pdf ? (
          failed ? (
            <p className="text-[13px] text-content/60">
              This PDF could not be drawn.
            </p>
          ) : doc && width ? (
            Array.from({ length: doc.numPages }, (_, index) => {
              const number = index + 1;
              return (
                <PaperPage
                  key={number}
                  doc={doc}
                  number={number}
                  width={width}
                  ratio={ratio}
                  eager={number === homePage || placed.byPage.has(number)}
                  passages={placed.byPage.get(number) ?? []}
                  onHighlight={onHighlight}
                />
              );
            })
          ) : (
            <div className="grid h-40 place-items-center text-content/50">
              <Loader className="size-4 animate-spin" />
            </div>
          )
        ) : (
          <div className="mx-auto max-w-[640px]">
            <h3 className="mb-2 text-[12px] font-medium tracking-wide text-content/50 uppercase">
              Abstract
            </h3>
            {paper.abstract ? (
              <>
                <Abstract
                  text={paper.abstract}
                  passages={cited}
                  focus={focus}
                />
                {paper.abstractFrom ? (
                  <p className="mt-3 text-[11px] text-content/45">
                    From {paper.abstractFrom}.
                  </p>
                ) : null}
                <p className="mt-4 rounded-md bg-content/5 px-3 py-2 text-[12px] leading-relaxed text-content/60">
                  {view.blocked ? (
                    <>
                      This paper may be free to read, but its site refuses
                      automatic downloads, so MonoBOT only has the abstract.
                      Open it with{" "}
                      <strong className="font-medium">Publisher</strong>,
                      download the PDF, then choose{" "}
                      <strong className="font-medium">Add PDF</strong>: you get
                      the paper itself with what answers cited underlined.
                    </>
                  ) : (
                    <>
                      No free copy of this paper's PDF was found, so only its
                      abstract is shown. If you have the PDF (through your
                      library, say), choose{" "}
                      <strong className="font-medium">Add PDF</strong>: it is
                      kept on this computer, and you get the paper itself with
                      what answers cited underlined.
                    </>
                  )}
                </p>
                {addError ? (
                  <p role="alert" className="mt-2 text-[12px] text-content/70">
                    {addError}
                  </p>
                ) : null}
              </>
            ) : (
              <p className="text-[13px] text-content/60">
                No source had this paper's abstract.
              </p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
