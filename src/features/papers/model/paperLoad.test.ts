import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
const extract = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("./pdfText", () => ({ extractPdfText: extract, loadPdfjs: vi.fn() }));

import { addPaperPdf, loadPaper } from "./paperStore";
import type { PaperDeps } from "./papers";

const LONG = "word ".repeat(120);

function openAlex(locations: string[], isOa = true) {
  return {
    id: "https://openalex.org/W1",
    doi: "https://doi.org/10.1234/open",
    title: "An open paper",
    publication_year: 2023,
    authorships: [{ author: { display_name: "Ada Lovelace" } }],
    primary_location: { landing_page_url: "https://doi.org/10.1234/open" },
    open_access: { is_oa: isOa },
    best_oa_location: null,
    abstract_inverted_index: null,
    locations: locations.map((url) => ({
      is_oa: true,
      landing_page_url: url,
      pdf_url: null,
    })),
  };
}

/** The scholarly APIs, answering the paper above and nothing else. */
function deps(work: unknown): PaperDeps {
  return {
    async api(url) {
      if (url.startsWith("https://api.openalex.org/works/doi:"))
        return JSON.stringify(work);
      throw new Error("down");
    },
  };
}

/** The native side: a cache of text files, and what `paper_pdf` does for each link. */
function native(
  links: Record<string, "pdf" | string>,
  files = new Map<string, string>(),
  exportFails = false,
) {
  const tried: string[] = [];
  const exported: Record<string, string>[] = [];
  invoke.mockImplementation(
    async (command: string, args: Record<string, string>) => {
      if (command === "paper_text_read") return files.get(args.key) ?? null;
      if (command === "paper_text_write")
        return void files.set(args.key, args.text);
      if (command === "paper_pdf") {
        tried.push(args.url);
        const result =
          links[args.url] ?? "That link is a web page, not the PDF";
        if (result === "pdf")
          return { path: "/cache/x.pdf", bytes: 9, cached: false };
        throw result;
      }
      if (command === "paper_pdf_bytes")
        return new Uint8Array([37, 80, 68, 70]).buffer;
      if (command === "paper_pdf_file")
        return new Uint8Array([37, 80, 68, 70, 1, 2]).buffer;
      if (command === "paper_export") {
        exported.push(args);
        if (exportFails) throw "Documents cannot be written";
        return `/Users/me/Documents/MonoBOT/${args.agent || "General"}/${args.name}.pdf`;
      }
      if (command === "paper_pdf_import")
        return { path: "/cache/y.pdf", bytes: 6, cached: false };
      throw new Error(`unmocked ${command}`);
    },
  );
  return { files, tried, exported };
}

beforeEach(() => {
  invoke.mockReset();
  extract.mockReset();
  extract.mockResolvedValue({ pages: [LONG, LONG] });
});

describe("loadPaper", () => {
  it("goes on to the next open copy when the first is not a PDF", async () => {
    const { tried, files } = native({
      "https://a.example/article": "That link is a web page, not the PDF",
      "https://repo.example/handle/1": "pdf",
    });
    const loaded = await loadPaper(
      { doi: "10.1234/open" },
      deps(
        openAlex([
          "https://a.example/article",
          "https://repo.example/handle/1",
        ]),
      ),
    );
    expect(loaded?.availability).toBe("full-text");
    expect(tried).toEqual([
      "https://a.example/article",
      "https://repo.example/handle/1",
    ]);
    expect(files.has("10.1234/open")).toBe(true);
  });

  it("says a paper is open but its site refuses programs, and keeps that for the viewer", async () => {
    const { files } = native({
      "https://doi.org/10.1234/open": "blocked:doi.org",
      "https://repo.example/handle/1": "blocked:repo.example",
    });
    const loaded = await loadPaper(
      { doi: "10.1234/open" },
      deps(
        openAlex([
          "https://doi.org/10.1234/open",
          "https://repo.example/handle/1",
        ]),
      ),
    );
    expect(loaded).toMatchObject({
      availability: "metadata-only",
      blocked: true,
    });
    expect(loaded?.reason).toContain(
      "This paper is open access, but the publisher's site and repo.example refuse automatic downloads",
    );
    expect(loaded?.reason).toContain("Add PDF");
    expect(JSON.parse(files.get("meta:10.1234/open")!)).toMatchObject({
      blocked: true,
    });
  });

  it("is a plain failure, not a block, when the copies simply are not PDFs", async () => {
    native({});
    const loaded = await loadPaper(
      { doi: "10.1234/open" },
      deps(openAlex(["https://repo.example/page"])),
    );
    expect(loaded?.blocked).toBeUndefined();
    expect(loaded?.reason).toContain("web page");
  });

  it("answers a paper already read from the cache, without asking any service", async () => {
    const files = new Map<string, string>([
      [
        `meta:10.1234/open`,
        JSON.stringify({
          paper: {
            key: "10.1234/open",
            title: "Cached",
            authors: [],
            pdfUrls: [],
            access: "open",
          },
          availability: "full-text",
        }),
      ],
      [`10.1234/open`, JSON.stringify({ pages: [LONG] })],
    ]);
    native({}, files);
    const offline: PaperDeps = {
      api: vi.fn(async () => {
        throw new Error("no network");
      }),
    };
    const loaded = await loadPaper({ doi: "10.1234/open" }, offline);
    expect(loaded).toMatchObject({ availability: "full-text", cached: true });
    expect(loaded?.paper.title).toBe("Cached");
    expect(offline.api).not.toHaveBeenCalled();
  });
});

describe("addPaperPdf", () => {
  const FIRST_PAGE = [
    "Journal of Hydrology: Regional Studies 46 (2023) 101381",
    "Construction of a daily streamflow dataset for Peru using a similarity-based regionalization approach",
    "https://doi.org/10.1016/j.ejrh.2023.101381",
    "Abstract",
    "A total of 11,913 sub-catchments in Peru and transboundary catchments with neighboring countries in South America.",
    "Keywords: Peru",
    LONG,
  ].join("\n");

  it("names a PDF by the DOI on its first page and takes the paper's details from the services", async () => {
    const { files } = native({});
    extract.mockResolvedValue({ pages: [FIRST_PAGE] });
    const work = {
      ...openAlex([]),
      doi: "https://doi.org/10.1016/j.ejrh.2023.101381",
      title: "Construction of a daily streamflow dataset for Peru",
    };
    const loaded = await addPaperPdf(
      "/Users/me/Downloads/llauca.pdf",
      {},
      deps(work),
    );
    expect(loaded.paper.key).toBe("10.1016/j.ejrh.2023.101381");
    expect(loaded.availability).toBe("full-text");
    expect(invoke).toHaveBeenCalledWith("paper_pdf_import", {
      key: "10.1016/j.ejrh.2023.101381",
      path: "/Users/me/Downloads/llauca.pdf",
    });
    expect(
      JSON.parse(files.get("10.1016/j.ejrh.2023.101381")!).pages,
    ).toHaveLength(1);
    expect(
      JSON.parse(files.get("meta:10.1016/j.ejrh.2023.101381")!).availability,
    ).toBe("full-text");
  });

  it("keeps a PDF with no DOI under a fingerprint, with a title and abstract read from it", async () => {
    native({});
    extract.mockResolvedValue({
      pages: [FIRST_PAGE.replace(/https:\/\/doi.org\S+/, "")],
    });
    const loaded = await addPaperPdf("/tmp/x.pdf", {}, deps(openAlex([])));
    expect(loaded.paper.key).toMatch(/^pdf:[0-9a-f]{16}$/);
    expect(loaded.paper.title).toContain(
      "Construction of a daily streamflow dataset",
    );
    expect(loaded.paper.abstract).toContain("11,913 sub-catchments");
    // The same file is the same paper.
    const again = await addPaperPdf(
      "/elsewhere/copy.pdf",
      {},
      deps(openAlex([])),
    );
    expect(again.paper.key).toBe(loaded.paper.key);
  });

  it("is the copy of the paper it is told, and takes the title it is given", async () => {
    native({});
    const loaded = await addPaperPdf(
      "/tmp/p.pdf",
      { ref: { doi: "10.1234/open" }, title: "My title" },
      deps(openAlex([], false)),
    );
    expect(loaded.paper.key).toBe("10.1234/open");
    expect(loaded.paper.title).toBe("My title");
    await expect(
      addPaperPdf(
        "/tmp/p.pdf",
        { ref: { doi: "10.9999/nothing" } },
        {
          api: async () => {
            throw new Error("404");
          },
        },
      ),
    ).rejects.toThrow("No paper was found for that ref");
  });

  it("keeps a scan to be seen, and says it cannot be read", async () => {
    const { files } = native({});
    extract.mockResolvedValue({ pages: ["", ""] });
    const loaded = await addPaperPdf(
      "/tmp/scan.pdf",
      { title: "A scan" },
      deps(openAlex([])),
    );
    expect(loaded.availability).toBe("metadata-only");
    expect(loaded.reason).toContain("scan with no text layer");
    expect(files.has(loaded.paper.key)).toBe(false);
  });

  it("refuses what the native side says is not a PDF", async () => {
    native({});
    invoke.mockImplementationOnce(async () => {
      throw "That file is not a PDF";
    });
    await expect(
      addPaperPdf("/tmp/page.pdf", {}, deps(openAlex([]))),
    ).rejects.toBe("That file is not a PDF");
  });
});

describe("the copy in Documents", () => {
  const OPEN = deps(openAlex(["https://repo.example/handle/1"]));
  const LINKS = { "https://repo.example/handle/1": "pdf" as const };

  it("puts a copy in the folder of the agent that read the paper and remembers where", async () => {
    const { exported, files } = native(LINKS);
    const loaded = await loadPaper({ doi: "10.1234/open" }, OPEN, {
      agent: "Research",
    });
    expect(exported).toEqual([
      {
        key: "10.1234/open",
        agent: "Research",
        name: "Lovelace (2023) - An open paper",
      },
    ]);
    expect(loaded?.file).toBe(
      "/Users/me/Documents/MonoBOT/Research/Lovelace (2023) - An open paper.pdf",
    );
    expect(JSON.parse(files.get("meta:10.1234/open")!)).toMatchObject({
      file: loaded?.file,
      agent: "Research",
    });
  });

  it("keeps a paper with no agent in the shared folder", async () => {
    const { exported } = native(LINKS);
    const loaded = await loadPaper({ doi: "10.1234/open" }, OPEN);
    expect(exported[0].agent).toBe("");
    expect(loaded?.file).toContain("/MonoBOT/General/");
  });

  it("still reads the paper when Documents cannot be written", async () => {
    native(LINKS, new Map(), true);
    const loaded = await loadPaper({ doi: "10.1234/open" }, OPEN, {
      agent: "Research",
    });
    expect(loaded?.availability).toBe("full-text");
    expect(loaded?.file).toBeUndefined();
  });

  it("copies nothing for a paper that was only its abstract", async () => {
    const { exported } = native({});
    await loadPaper({ doi: "10.1234/open" }, deps(openAlex([], false)), {
      agent: "Research",
    });
    expect(exported).toEqual([]);
  });

  it("gives a PDF the user added a copy too, under the agent that took it", async () => {
    const { exported } = native({});
    const loaded = await addPaperPdf(
      "/tmp/p.pdf",
      { ref: { doi: "10.1234/open" }, agent: "Banana" },
      deps(openAlex([], false)),
    );
    expect(exported[0]).toMatchObject({ agent: "Banana", key: "10.1234/open" });
    expect(loaded.file).toContain("/MonoBOT/Banana/");
  });
});
