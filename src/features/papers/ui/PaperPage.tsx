import { useEffect, useRef, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
import { locateInItems } from "../model/locate";
import { loadPdfjs } from "../model/pdfText";
import { clearHighlight, drawHighlights } from "./highlight";

/** Where a cited passage ended up on a page, or that it was not found there. */
export type PageHit = { passage: string; range?: Range };

/**
 * One page of a paper: its canvas and, above it, its text. It draws when it
 * comes near the screen, so a long paper opens at once. On a page that cited
 * passages point to, it underlines the quoted words of all of them.
 */
export function PaperPage({
  doc,
  number,
  width,
  ratio,
  passages,
  eager,
  onHighlight,
}: {
  doc: PDFDocumentProxy;
  number: number;
  width: number;
  /** Height over width, from the first page, until this one is measured. */
  ratio: number;
  /** The cited passages to underline on this page. */
  passages: readonly string[];
  /** Draw now, even off screen: a page a citation points to. */
  eager?: boolean;
  onHighlight?: (page: number, hits: PageHit[]) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const layer = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(!!eager);
  const [height, setHeight] = useState(Math.round(width * ratio));
  const wanted = passages.join("\u0000");

  useEffect(() => {
    if (eager) setNear(true);
  }, [eager]);

  useEffect(() => {
    if (near || !host.current) return;
    const watch = new IntersectionObserver(
      ([entry]) => entry?.isIntersecting && setNear(true),
      { rootMargin: "800px 0px" },
    );
    watch.observe(host.current);
    return () => watch.disconnect();
  }, [near]);

  useEffect(() => setHeight(Math.round(width * ratio)), [width, ratio]);

  useEffect(() => {
    if (!near || width < 50) return;
    let live = true;
    let cancel: (() => void) | undefined;
    let marked: HTMLElement[] = [];
    const owner = layer.current;
    const quoted = wanted ? wanted.split("\u0000") : [];
    void (async () => {
      const [pdfjs, page] = await Promise.all([
        loadPdfjs(),
        doc.getPage(number),
      ]);
      if (!live || !canvas.current || !layer.current) return;
      const base = page.getViewport({ scale: 1 });
      const viewport = page.getViewport({ scale: width / base.width });
      const dpr = window.devicePixelRatio || 1;
      const el = canvas.current;
      el.width = Math.floor(viewport.width * dpr);
      el.height = Math.floor(viewport.height * dpr);
      el.style.width = `${Math.floor(viewport.width)}px`;
      el.style.height = `${Math.floor(viewport.height)}px`;
      setHeight(Math.floor(viewport.height));
      const context = el.getContext("2d");
      if (!context) return;
      const painting = page.render({
        canvasContext: context,
        viewport,
        transform: dpr === 1 ? undefined : [dpr, 0, 0, dpr, 0, 0],
      });
      const text = layer.current;
      text.replaceChildren();
      text.style.setProperty("--scale-factor", String(viewport.scale));
      const words = new pdfjs.TextLayer({
        textContentSource: page.streamTextContent(),
        container: text,
        viewport,
      });
      cancel = () => {
        painting.cancel();
        words.cancel();
      };
      try {
        await Promise.all([painting.promise, words.render()]);
      } catch {
        return; // cancelled by a resize or by closing the paper
      }
      if (!live || !quoted.length) return;
      marked = words.textDivs.filter((div) => div.isConnected);
      const items = marked.map((div) => div.textContent ?? "");
      const found = quoted.map((passage) => ({
        passage,
        at: locateInItems(items, passage),
      }));
      const drawn = drawHighlights(
        text,
        marked,
        found.flatMap(({ at }) => (at ? [at] : [])),
      );
      let next = 0;
      onHighlight?.(
        number,
        found.map(({ passage, at }) => {
          if (!at) return { passage };
          const range = drawn[next];
          next += 1;
          return range ? { passage, range } : { passage };
        }),
      );
    })().catch(() => undefined);
    return () => {
      live = false;
      cancel?.();
      if (quoted.length && owner) clearHighlight(owner, marked);
    };
  }, [near, width, doc, number, wanted, onHighlight]);

  return (
    <div
      ref={host}
      data-paper-page={number}
      className="paper-page relative mx-auto mb-3 overflow-hidden rounded-sm bg-white shadow-[0_0_0_1px_rgb(0_0_0/0.08)]"
      style={{ width, height }}
    >
      <canvas ref={canvas} className="block" />
      <div ref={layer} className="textLayer" />
    </div>
  );
}
