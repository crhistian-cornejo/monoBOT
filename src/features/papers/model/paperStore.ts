import { invoke } from "@tauri-apps/api/core";
import type { Paper, PaperDeps, PaperRef } from "./papers";
import { findPapers, paperLabel, resolvePaper } from "./papers";
import {
  detectDoi,
  guessAbstract,
  guessTitle,
  type PaperText,
} from "./paperText";
import { extractPdfText, loadPdfjs } from "./pdfText";

/** The open APIs, through the app's native side (the webview's CSP is narrow). */
export const nativePaperDeps: PaperDeps = {
  api: (url) => invoke<string>("paper_api", { url }),
};

export type PaperAvailability =
  /** The PDF was downloaded to the cache and its text read. */
  | "full-text"
  /** Only the abstract: no open copy exists, or none could be read. */
  | "abstract-only"
  | "metadata-only";

export type LoadedPaper = {
  paper: Paper;
  availability: PaperAvailability;
  text?: PaperText;
  /** Why the full text is missing, for the agent to tell the user. */
  reason?: string;
  /** A site refused the program: the user can still fetch the PDF themselves. */
  blocked?: boolean;
  /** Where a copy of the PDF is in the user's Documents, in the agent's folder. */
  file?: string;
  /** The PDF is in the cache and the app can show it. */
  cached: boolean;
};

type CachedPdf = { path: string; bytes: number; cached: boolean };

async function cachedText(key: string): Promise<PaperText | undefined> {
  const raw = await invoke<string | null>("paper_text_read", { key });
  if (!raw) return undefined;
  try {
    const parsed = JSON.parse(raw) as PaperText;
    return Array.isArray(parsed.pages) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Finds the paper and, when an open copy exists, downloads it to the cache
 * (never to Downloads) and reads its text. A paper read once is summarised
 * from the cache from then on.
 */
export async function loadPaper(
  ref: PaperRef,
  deps: PaperDeps = nativePaperDeps,
  options: { wantText?: boolean; agent?: string } = {},
): Promise<LoadedPaper | undefined> {
  const loaded = await loadPaperUncached(ref, deps, options);
  if (loaded) {
    await exportCopy(loaded, options.agent);
    await remember(loaded, options.agent).catch(() => undefined);
  }
  return loaded;
}

/**
 * Puts a copy of a paper's PDF in the user's Documents (macOS and Windows
 * alike), in a folder for the agent that read it: Documents/MonoBOT/<agent>.
 * The cache keeps the working copy; this one is the user's to open and keep.
 */
async function exportCopy(loaded: LoadedPaper, agent?: string) {
  if (loaded.availability !== "full-text") return;
  try {
    loaded.file = await invoke<string>("paper_export", {
      key: loaded.paper.key,
      agent: agent ?? "",
      name: `${paperLabel(loaded.paper)} - ${loaded.paper.title}`,
    });
  } catch {
    // The folder cannot be written: the paper still reads from the cache.
  }
}

/** What the viewer needs besides the PDF and its text: who the paper is. */
type PaperMeta = {
  paper: Paper;
  availability: PaperAvailability;
  reason?: string;
  blocked?: boolean;
  /** The copy in Documents, and the agent whose folder it is in. */
  file?: string;
  agent?: string;
};

const metaKey = (key: string) => `meta:${key}`;

async function remember(loaded: LoadedPaper, agent?: string) {
  const meta: PaperMeta = {
    paper: loaded.paper,
    availability: loaded.availability,
    ...(loaded.reason ? { reason: loaded.reason } : {}),
    ...(loaded.blocked ? { blocked: true } : {}),
    ...(loaded.file ? { file: loaded.file } : {}),
    ...(agent ? { agent } : {}),
  };
  await invoke("paper_text_write", {
    key: metaKey(loaded.paper.key),
    text: JSON.stringify(meta),
  });
}

export type PaperView = PaperMeta & {
  text?: PaperText;
  /** The cached PDF, when one was downloaded: the app draws it. */
  pdf?: Uint8Array;
};

/** Whether a paper was read before, without loading its PDF. */
export async function paperKnown(key: string): Promise<boolean> {
  const raw = await invoke<string | null>("paper_text_read", {
    key: metaKey(key),
  }).catch(() => null);
  return !!raw;
}

/** A paper already read, for the viewer; undefined when it never was. */
export async function readPaperView(
  key: string,
): Promise<PaperView | undefined> {
  const raw = await invoke<string | null>("paper_text_read", {
    key: metaKey(key),
  }).catch(() => null);
  if (!raw) return undefined;
  let meta: PaperMeta;
  try {
    meta = JSON.parse(raw) as PaperMeta;
  } catch {
    return undefined;
  }
  if (!meta?.paper?.title) return undefined;
  const text = await cachedText(key).catch(() => undefined);
  const pdf = await readPaperPdf(key);
  return { ...meta, ...(text ? { text } : {}), ...(pdf ? { pdf } : {}) };
}

/** The cached PDF's bytes, or undefined when none was downloaded. */
export async function readPaperPdf(
  key: string,
): Promise<Uint8Array | undefined> {
  try {
    return new Uint8Array(
      await invoke<ArrayBuffer>("paper_pdf_bytes", { key }),
    );
  } catch {
    return undefined;
  }
}

/** The cache key a reference names without asking any service, when it does. */
function cachedKeyOf(ref: PaperRef): string | undefined {
  if ("cached" in ref) return ref.cached;
  if ("doi" in ref) return ref.doi;
  if ("arxiv" in ref) return `10.48550/arxiv.${ref.arxiv.toLowerCase()}`;
  return undefined;
}

/** Sites that only forward to the publisher: their name tells nothing. */
const RESOLVERS = new Set(["doi.org", "dx.doi.org", "hdl.handle.net"]);

function blockedReason(hosts: Set<string>, open: boolean): string {
  const names = [
    ...new Set(
      [...hosts].map((host) =>
        RESOLVERS.has(host) ? "the publisher's site" : host,
      ),
    ),
  ];
  const who = names.join(" and ");
  const refuse = names.length > 1 ? "refuse" : "refuses";
  return open
    ? `This paper is open access, but ${who} ${refuse} automatic downloads (a human check is asked for), so MonoBOT cannot fetch the PDF. Download it from the publisher's page and add it with Add PDF.`
    : `The only copies found are on ${who}, which ${refuse} automatic downloads. Download the PDF yourself and add it with Add PDF.`;
}

async function loadPaperUncached(
  ref: PaperRef,
  deps: PaperDeps,
  options: { wantText?: boolean },
): Promise<LoadedPaper | undefined> {
  // A paper already read, or kept from a PDF the user gave, needs no service.
  const known = cachedKeyOf(ref);
  if (known && options.wantText !== false) {
    const view = await readPaperView(known).catch(() => undefined);
    if (view?.text?.pages.some((page) => page.trim()))
      return {
        paper: view.paper,
        availability: "full-text",
        text: view.text,
        cached: true,
      };
  }
  const paper = await resolvePaper(ref, deps);
  if (!paper) return undefined;
  const base = { paper, cached: false };
  const fallback = (reason: string, blocked = false): LoadedPaper => ({
    ...base,
    availability: paper.abstract ? "abstract-only" : "metadata-only",
    reason,
    ...(blocked ? { blocked } : {}),
  });
  if (options.wantText === false) return fallback("not requested");
  const stored = await cachedText(paper.key).catch(() => undefined);
  if (stored?.pages.some((page) => page.trim()))
    return { ...base, availability: "full-text", text: stored, cached: true };
  if (!paper.pdfUrls.length)
    return fallback(
      paper.access === "closed"
        ? "It is not open access and no repository holds a free copy."
        : "No open copy of the PDF was found.",
    );
  let last = "";
  const blocked = new Set<string>();
  for (const url of paper.pdfUrls.slice(0, 8)) {
    try {
      await invoke<CachedPdf>("paper_pdf", {
        key: paper.key,
        url,
      });
      const bytes = new Uint8Array(
        await invoke<ArrayBuffer>("paper_pdf_bytes", { key: paper.key }),
      );
      const text = await extractPdfText(bytes);
      if (!text.pages.some((page) => page.trim().length > 200)) {
        last = "The PDF has no text layer (a scan).";
        continue;
      }
      await invoke("paper_text_write", {
        key: paper.key,
        text: JSON.stringify(text),
      }).catch(() => undefined);
      return {
        ...base,
        availability: "full-text",
        text,
        cached: true,
      };
    } catch (error) {
      const refusal = /^blocked:(.+)$/.exec(String(error));
      if (refusal) blocked.add(refusal[1]);
      else last = String(error);
    }
  }
  if (blocked.size)
    return fallback(blockedReason(blocked, paper.access === "open"), true);
  return fallback(
    last
      ? `Could not read an open copy: ${last}`
      : "No open copy of the PDF could be fetched.",
  );
}

export { findPapers };

function toBase64(bytes: Uint8Array): string {
  let binary = "";
  for (let at = 0; at < bytes.length; at += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(binary);
}

export type RenderedPage = {
  /** Where the PNG is, in the app's cache: it embeds in a chat reply. */
  path: string;
  page: number;
  pages: number;
};

/**
 * Draws one page of a cached paper as a PNG, in the cache folder, so a figure,
 * a table or an equation can be shown in the chat. It is the whole page: a
 * crop would need to know where the figure sits.
 */
export async function renderPaperPage(
  key: string,
  page: number,
  scale = 1.6,
): Promise<RenderedPage> {
  const bytes = await readPaperPdf(key);
  if (!bytes) throw new Error("This paper's PDF is not on this computer");
  const pdfjs = await loadPdfjs();
  const doc = await pdfjs.getDocument({ data: bytes.slice() }).promise;
  try {
    if (!Number.isInteger(page) || page < 1 || page > doc.numPages)
      throw new Error(`The paper has ${doc.numPages} pages`);
    const drawn = await doc.getPage(page);
    const viewport = drawn.getViewport({ scale });
    const canvas = document.createElement("canvas");
    canvas.width = Math.floor(viewport.width);
    canvas.height = Math.floor(viewport.height);
    const context = canvas.getContext("2d");
    if (!context) throw new Error("Could not draw the page");
    await drawn.render({ canvasContext: context, viewport }).promise;
    const blob = await new Promise<Blob | null>((done) =>
      canvas.toBlob(done, "image/png"),
    );
    if (!blob) throw new Error("Could not draw the page");
    const path = await invoke<string>("paper_image_write", {
      key,
      page,
      pngBase64: toBase64(new Uint8Array(await blob.arrayBuffer())),
    });
    return { path, page, pages: doc.numPages };
  } finally {
    await doc.destroy();
  }
}

/**
 * Keeps a PDF the user chose as this paper's copy: for a paper with no open
 * copy, one they got through their library. The app reads its text, so the
 * viewer can show it, underline what answers cited, and an agent can read it.
 */
export async function importPaperPdf(key: string, path: string): Promise<void> {
  await invoke("paper_pdf_import", { key, path });
  const bytes = await readPaperPdf(key);
  if (!bytes) throw new Error("The PDF could not be kept");
  const raw = await invoke<string | null>("paper_text_read", {
    key: metaKey(key),
  });
  const meta = raw ? (JSON.parse(raw) as PaperMeta) : undefined;
  const text = await extractPdfText(bytes);
  const readable = text.pages.some((page) => page.trim().length > 200);
  if (readable)
    await invoke("paper_text_write", { key, text: JSON.stringify(text) });
  if (!meta) return;
  const copy = readable
    ? await invoke<string>("paper_export", {
        key,
        agent: meta.agent ?? "",
        name: `${paperLabel(meta.paper)} - ${meta.paper.title}`,
      }).catch(() => undefined)
    : undefined;
  const next: PaperMeta = readable
    ? {
        paper: meta.paper,
        availability: "full-text",
        ...(copy ? { file: copy } : {}),
        ...(meta.agent ? { agent: meta.agent } : {}),
      }
    : {
        paper: meta.paper,
        availability: meta.availability,
        reason:
          "The PDF you added is a scan with no text layer: it can be seen but not searched or underlined.",
      };
  await invoke("paper_text_write", {
    key: metaKey(key),
    text: JSON.stringify(next),
  });
}

/** The key of a PDF with no DOI: a fingerprint of its bytes, so the same file is the same paper. */
async function fingerprint(bytes: Uint8Array): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", bytes.slice().buffer);
  return `pdf:${[...new Uint8Array(digest).slice(0, 8)]
    .map((byte) => byte.toString(16).padStart(2, "0"))
    .join("")}`;
}

/**
 * Registers a PDF the user gave (attached to a chat, or on disk) as a paper:
 * it is kept in the cache and its text read, so it can be read by sections,
 * drawn, cited and underlined like a paper the app downloaded. With `ref` it
 * is the copy of that paper; otherwise its DOI, printed on its first page,
 * names it, and a PDF with none is kept under a fingerprint of the file.
 */
export async function addPaperPdf(
  path: string,
  options: { ref?: PaperRef; title?: string; agent?: string } = {},
  deps: PaperDeps = nativePaperDeps,
): Promise<LoadedPaper> {
  const bytes = new Uint8Array(
    await invoke<ArrayBuffer>("paper_pdf_file", { path }),
  );
  const text = await extractPdfText(bytes);
  const readable = text.pages.some((page) => page.trim().length > 200);
  const doi = options.ref ? undefined : readable ? detectDoi(text) : undefined;
  let paper: Paper | undefined;
  if (options.ref) {
    paper = await resolvePaper(options.ref, deps).catch(() => undefined);
    if (!paper)
      throw new Error(
        "No paper was found for that ref; leave it out to register the PDF on its own.",
      );
  } else if (doi) {
    paper = await resolvePaper({ doi }, deps).catch(() => undefined);
  }
  if (!paper) {
    const abstract = readable ? guessAbstract(text) : undefined;
    paper = {
      key: doi ?? (await fingerprint(bytes)),
      ...(doi ? { doi } : {}),
      title:
        options.title?.trim() ||
        (readable ? guessTitle(text) : undefined) ||
        path
          .split(/[\\/]/)
          .pop()
          ?.replace(/\.pdf$/i, "") ||
        "Untitled PDF",
      authors: [],
      pdfUrls: [],
      access: "unknown",
      ...(abstract ? { abstract, abstractFrom: "the PDF" } : {}),
    };
  } else if (options.title?.trim()) {
    paper = { ...paper, title: options.title.trim() };
  }
  await invoke("paper_pdf_import", { key: paper.key, path });
  if (readable)
    await invoke("paper_text_write", {
      key: paper.key,
      text: JSON.stringify(text),
    });
  const loaded: LoadedPaper = readable
    ? { paper, availability: "full-text", text, cached: true }
    : {
        paper,
        availability: paper.abstract ? "abstract-only" : "metadata-only",
        reason:
          "The PDF is a scan with no text layer: it can be seen but not read, searched or underlined.",
        cached: true,
      };
  await exportCopy(loaded, options.agent);
  await remember(loaded, options.agent).catch(() => undefined);
  return loaded;
}
