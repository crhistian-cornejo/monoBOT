import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
const extract = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("./pdfText", () => ({
  extractPdfText: extract,
  loadPdfjs: vi.fn(),
}));

import { importPaperPdf } from "./paperStore";

const KEY = "10.1016/j.envsoft.2017.05.002";
const META = {
  paper: {
    key: KEY,
    title: "The suite of lumped GR models",
    authors: [],
    pdfUrls: [],
    access: "closed",
  },
  availability: "abstract-only",
  reason: "It is not open access and no repository holds a free copy.",
};

/** The cache as the commands see it: text files by key, and the PDF bytes. */
function cache(options: { meta?: unknown; pdf?: boolean } = {}) {
  const files = new Map<string, string>();
  if (options.meta !== null)
    files.set(`meta:${KEY}`, JSON.stringify(options.meta ?? META));
  invoke.mockImplementation(
    async (command: string, args: Record<string, string>) => {
      if (command === "paper_pdf_import")
        return { path: "/cache/x.pdf", bytes: 4, cached: false };
      if (command === "paper_pdf_bytes") {
        if (options.pdf === false) throw new Error("none");
        return new Uint8Array([37, 80, 68, 70]).buffer;
      }
      if (command === "paper_text_read") return files.get(args.key) ?? null;
      if (command === "paper_text_write")
        return void files.set(args.key, args.text);
      throw new Error(`unmocked ${command}`);
    },
  );
  return files;
}

beforeEach(() => {
  invoke.mockReset();
  extract.mockReset();
});

describe("importPaperPdf", () => {
  it("keeps the PDF, its text, and turns the paper into a full-text one", async () => {
    const files = cache();
    extract.mockResolvedValue({ pages: ["x".repeat(400), "y".repeat(400)] });
    await importPaperPdf(KEY, "/Users/me/Downloads/paper.pdf");
    expect(invoke).toHaveBeenCalledWith("paper_pdf_import", {
      key: KEY,
      path: "/Users/me/Downloads/paper.pdf",
    });
    expect(JSON.parse(files.get(KEY)!).pages).toHaveLength(2);
    expect(JSON.parse(files.get(`meta:${KEY}`)!)).toEqual({
      paper: META.paper,
      availability: "full-text",
    });
  });

  it("keeps a scan to be seen, and says it cannot be searched or underlined", async () => {
    const files = cache();
    extract.mockResolvedValue({ pages: ["", " "] });
    await importPaperPdf(KEY, "/scan.pdf");
    expect(files.has(KEY)).toBe(false);
    const meta = JSON.parse(files.get(`meta:${KEY}`)!);
    expect(meta.availability).toBe("abstract-only");
    expect(meta.reason).toContain("scan with no text layer");
  });

  it("passes on the refusal of a file that is not a PDF", async () => {
    cache();
    invoke.mockImplementationOnce(async () => {
      throw "That file is not a PDF";
    });
    await expect(importPaperPdf(KEY, "/page.pdf")).rejects.toBe(
      "That file is not a PDF",
    );
  });

  it("still keeps the text of a paper whose record is missing", async () => {
    const files = cache({ meta: null });
    extract.mockResolvedValue({ pages: ["z".repeat(500)] });
    await importPaperPdf(KEY, "/p.pdf");
    expect(files.has(KEY)).toBe(true);
    expect(files.has(`meta:${KEY}`)).toBe(false);
  });
});
