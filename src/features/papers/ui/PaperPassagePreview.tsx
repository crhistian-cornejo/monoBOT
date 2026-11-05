import { useEffect, useState } from "react";
import type { PDFDocumentProxy } from "pdfjs-dist/legacy/build/pdf.mjs";
import { locateInItems, locatePage, rangeParts } from "../model/locate";
import { paperKeyOf } from "../model/openPaper";
import { cropAround, partBoxes, type Box, type ItemBox } from "../model/passageCrop";
import { readPaperView } from "../model/paperStore";
import { loadPdfjs } from "../model/pdfText";

/** The part of a page a passage is on, drawn, with the passage marked on it. */
type Crop = {
  src: string;
  page: number;
  /** Size in CSS pixels at the width it was drawn for. */
  width: number;
  height: number;
  /** The passage's boxes, in CSS pixels over the picture. */
  marks: Box[];
};

type OpenPaper = { doc: PDFDocumentProxy; pages: readonly string[] };

// A few papers stay open, so moving between the citations of one answer does
// not read and parse the same PDF again; the oldest closes past that.
const OPEN_LIMIT = 3;
const opened = new Map<string, Promise<OpenPaper | undefined>>();
const CROP_LIMIT = 40;
const crops = new Map<string, Promise<Crop | undefined>>();

/** Keeps `value` as the newest entry, dropping the oldest past `limit`. */
function remember<T>(
  cache: Map<string, T>,
  key: string,
  value: T,
  limit: number,
  evict?: (value: T) => void,
) {
  cache.delete(key);
  cache.set(key, value);
  while (cache.size > limit) {
    const [oldest, dropped] = cache.entries().next().value as [string, T];
    cache.delete(oldest);
    evict?.(dropped);
  }
}

const closePaper = (paper: Promise<OpenPaper | undefined>) =>
  void paper.then((open) => open?.doc.destroy());

function openPaperPdf(key: string): Promise<OpenPaper | undefined> {
  const known = opened.get(key);
  if (known) {
    remember(opened, key, known, OPEN_LIMIT, closePaper);
    return known;
  }
  const loading = (async () => {
    const view = await readPaperView(key);
    if (!view?.pdf || !view.text) return undefined;
    const pdfjs = await loadPdfjs();
    // pdf.js takes the buffer over, so it gets its own copy.
    const doc = await pdfjs.getDocument({ data: view.pdf.slice() }).promise;
    return { doc, pages: view.text.pages };
  })().catch(() => undefined);
  remember(opened, key, loading, OPEN_LIMIT, closePaper);
  // A paper that could not be opened is tried again next time.
  void loading.then((paper) => {
    if (!paper && opened.get(key) === loading) opened.delete(key);
  });
  return loading;
}

async function drawCrop(
  key: string,
  passage: string,
  width: number,
): Promise<Crop | undefined> {
  const paper = await openPaperPdf(key);
  if (!paper) return undefined;
  const number = locatePage(paper.pages, passage)?.page;
  if (!number || number > paper.doc.numPages) return undefined;
  const [pdfjs, page] = await Promise.all([
    loadPdfjs(),
    paper.doc.getPage(number),
  ]);
  const base = page.getViewport({ scale: 1 });
  const content = await page.getTextContent();
  const items: ItemBox[] = [];
  const words: string[] = [];
  for (const item of content.items) {
    if (!("str" in item)) continue;
    const at = pdfjs.Util.transform(base.transform, item.transform);
    const height = Math.hypot(at[2], at[3]);
    items.push({
      box: { x: at[4], y: at[5] - height, width: item.width, height },
      length: item.str.length,
    });
    words.push(item.str);
  }
  const found = locateInItems(words, passage);
  if (!found) return undefined;
  const marks = partBoxes(items, rangeParts(words, found));
  const crop = cropAround(marks, base);
  if (!crop || crop.width <= 0 || crop.height <= 0) return undefined;
  const dpr = window.devicePixelRatio || 1;
  const scale = (width * dpr) / crop.width;
  const viewport = page.getViewport({ scale });
  const full = document.createElement("canvas");
  full.width = Math.ceil(viewport.width);
  full.height = Math.ceil(viewport.height);
  const fullContext = full.getContext("2d");
  if (!fullContext) return undefined;
  await page.render({ canvasContext: fullContext, viewport }).promise;
  const out = document.createElement("canvas");
  out.width = Math.round(crop.width * scale);
  out.height = Math.round(crop.height * scale);
  out
    .getContext("2d")
    ?.drawImage(
      full,
      crop.x * scale,
      crop.y * scale,
      out.width,
      out.height,
      0,
      0,
      out.width,
      out.height,
    );
  const css = width / crop.width;
  return {
    src: out.toDataURL("image/png"),
    page: number,
    width,
    height: crop.height * css,
    marks: marks.map((mark) => ({
      x: (mark.x - crop.x) * css,
      y: (mark.y - crop.y) * css,
      width: mark.width * css,
      height: mark.height * css,
    })),
  };
}

function passageCrop(key: string, passage: string, width: number) {
  const id = `${key}\u0000${passage}\u0000${width}`;
  const known = crops.get(id);
  if (known) return known;
  const drawing = drawCrop(key, passage, width).catch(() => undefined);
  remember(crops, id, drawing, CROP_LIMIT);
  return drawing;
}

/**
 * Where a citation's passage is in the paper: a picture of that part of the
 * page with the quoted words marked, as the paper's viewer underlines them.
 * Nothing shows for a source that is not a paper the app has the PDF of.
 */
export function PaperPassagePreview({
  href,
  passage,
  width,
}: {
  href: string;
  passage?: string;
  width: number;
}) {
  const key = paperKeyOf(href);
  const [crop, setCrop] = useState<Crop>();
  useEffect(() => {
    setCrop(undefined);
    if (!key || !passage) return;
    let live = true;
    void passageCrop(key, passage, width).then(
      (drawn) => live && setCrop(drawn),
    );
    return () => {
      live = false;
    };
  }, [key, passage, width]);
  if (!crop) return null;
  return (
    <figure className="mt-2">
      <div
        className="relative overflow-hidden rounded-md bg-white shadow-[0_0_0_1px_rgb(0_0_0/0.08)]"
        style={{ width: crop.width, height: crop.height }}
      >
        <img
          src={crop.src}
          alt=""
          className="block"
          style={{ width: crop.width, height: crop.height }}
        />
        {crop.marks.map((mark, index) => (
          <span
            key={index}
            aria-hidden
            className="absolute rounded-[1px] bg-accent/30 mix-blend-multiply"
            style={{
              left: mark.x,
              top: mark.y,
              width: mark.width,
              height: mark.height,
            }}
          />
        ))}
      </div>
      <figcaption className="mt-1 text-[11px] text-content/50">
        Page {crop.page} · click to open the paper here
      </figcaption>
    </figure>
  );
}
