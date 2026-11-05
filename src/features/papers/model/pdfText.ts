import type { PaperText } from "./paperText";

/**
 * Reads a PDF's text page by page in the webview with pdf.js, loaded only when
 * a paper is opened. Text-layer PDFs only: a scanned paper has none.
 */
/** pdf.js with its worker, loaded the first time a PDF is opened. */
export async function loadPdfjs() {
  const pdfjs = await import("pdfjs-dist/legacy/build/pdf.mjs");
  if (!pdfjs.GlobalWorkerOptions.workerSrc) {
    pdfjs.GlobalWorkerOptions.workerSrc = new URL(
      "pdfjs-dist/legacy/build/pdf.worker.min.mjs",
      import.meta.url,
    ).toString();
  }
  return pdfjs;
}

export async function extractPdfText(
  data: Uint8Array,
  maxPages = 80,
): Promise<PaperText> {
  const pdfjs = await loadPdfjs();
  const doc = await pdfjs.getDocument({ data }).promise;
  const pages: string[] = [];
  try {
    for (
      let number = 1;
      number <= Math.min(doc.numPages, maxPages);
      number += 1
    ) {
      const page = await doc.getPage(number);
      const content = await page.getTextContent();
      let line = "";
      let out = "";
      for (const item of content.items) {
        if (!("str" in item)) continue;
        line += item.str;
        if (item.hasEOL) {
          out += `${line}\n`;
          line = "";
        } else if (item.str && !item.str.endsWith(" ")) line += " ";
      }
      pages.push(`${out}${line}`.replace(/-\n(?=\p{Ll})/gu, "").trim());
    }
  } finally {
    await doc.destroy();
  }
  return { pages };
}
