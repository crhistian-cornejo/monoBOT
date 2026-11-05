import { describe, expect, it } from "vitest";
import {
  abstractFromIndex,
  arxivId,
  findPapers,
  isLikelyPdfUrl,
  normalizeDoi,
  parseEuropePmc,
  paperLabel,
  parseCrossrefItem,
  parseHalDoc,
  parseOpenAlexWork,
  parseRef,
  resolvePaper,
  stripJats,
  type PaperDeps,
} from "./papers";
import {
  bestPassages,
  detectDoi,
  guessAbstract,
  guessTitle,
  excerpt,
  figures,
  outline,
  parsePages,
  section,
} from "./paperText";

const ABSTRACT =
  "Lumped hydrological models are catchment-scale representations of the transformation of precipitation into discharge.";

/** An API that answers by the longest matching prefix, else fails. */
function deps(
  replies: Record<string, unknown>,
): PaperDeps & { calls: string[] } {
  const calls: string[] = [];
  return {
    calls,
    async api(url) {
      calls.push(url);
      const key = Object.keys(replies)
        .filter((prefix) => url.startsWith(prefix))
        .sort((a, b) => b.length - a.length)[0];
      if (!key) throw new Error(`no reply for ${url}`);
      const reply = replies[key];
      if (reply instanceof Error) throw reply;
      return typeof reply === "string" ? reply : JSON.stringify(reply);
    },
  };
}

describe("identifiers", () => {
  it("finds a DOI in a bare id, a link or doi: text", () => {
    expect(normalizeDoi("10.1016/J.ENVSOFT.2017.05.002")).toBe(
      "10.1016/j.envsoft.2017.05.002",
    );
    expect(normalizeDoi("https://doi.org/10.1016/j.envsoft.2017.05.002.")).toBe(
      "10.1016/j.envsoft.2017.05.002",
    );
    expect(normalizeDoi("doi: 10.29007/qsqj")).toBe("10.29007/qsqj");
    expect(normalizeDoi("the suite of lumped GR models")).toBeUndefined();
  });

  it("reads arXiv ids and links, with or without a version", () => {
    expect(arxivId("2401.01234v2")).toBe("2401.01234");
    expect(arxivId("https://arxiv.org/abs/2401.01234")).toBe("2401.01234");
    expect(arxivId("arXiv:hep-th/9901001")).toBe("hep-th/9901001");
    expect(arxivId("hydrology in R")).toBeUndefined();
  });

  it("sorts a reference into the id it is", () => {
    expect(parseRef("https://doi.org/10.29007/qsqj")).toEqual({
      doi: "10.29007/qsqj",
    });
    expect(parseRef("2401.01234")).toEqual({ arxiv: "2401.01234" });
    expect(parseRef("https://openalex.org/W2614679811")).toEqual({
      openAlex: "W2614679811",
    });
    expect(parseRef("GR4J in R")).toEqual({ title: "GR4J in R" });
  });
});

describe("abstracts", () => {
  it("puts OpenAlex's inverted index back in order", () => {
    const index: Record<string, number[]> = {};
    ABSTRACT.split(" ").forEach((word, position) => {
      index[word] = [...(index[word] ?? []), position];
    });
    expect(abstractFromIndex(index)).toBe(ABSTRACT);
  });

  it("treats an empty index as no abstract", () => {
    expect(abstractFromIndex({})).toBeUndefined();
    expect(abstractFromIndex(null)).toBeUndefined();
  });

  it("strips Crossref's JATS markup", () => {
    expect(
      stripJats(
        `<jats:p><jats:title>Abstract</jats:title>${ABSTRACT} See <jats:italic>airGR</jats:italic> &amp; more.</jats:p>`,
      ),
    ).toBe(`${ABSTRACT} See airGR & more.`);
  });
});

describe("parsers", () => {
  it("reads an OpenAlex work with its open copies, best first", () => {
    const paper = parseOpenAlexWork({
      id: "https://openalex.org/W1",
      doi: "https://doi.org/10.1234/abc",
      title: "A paper",
      publication_year: 2020,
      cited_by_count: 12,
      authorships: [
        { author: { display_name: "Ada Lovelace" } },
        { author: { display_name: "Alan Turing" } },
        { author: { display_name: "Grace Hopper" } },
      ],
      primary_location: {
        landing_page_url: "https://pub.example/abc",
        source: { display_name: "Journal of Tests" },
      },
      best_oa_location: { pdf_url: "https://repo.example/abc.pdf" },
      open_access: { is_oa: true, oa_url: "https://repo.example/abc.pdf" },
      locations: [
        { landing_page_url: "https://arxiv.org/abs/2001.00001", pdf_url: null },
      ],
    });
    expect(paper).toMatchObject({
      key: "10.1234/abc",
      doi: "10.1234/abc",
      openAlexId: "W1",
      venue: "Journal of Tests",
      access: "open",
      pdfUrls: [
        "https://repo.example/abc.pdf",
        "https://arxiv.org/pdf/2001.00001",
      ],
    });
    expect(paperLabel(paper!)).toBe("Lovelace et al. (2020)");
  });

  it("reads a Crossref item and a HAL record", () => {
    expect(
      parseCrossrefItem({
        DOI: "10.1234/ABC",
        title: ["A paper"],
        author: [{ given: "Ada", family: "Lovelace" }],
        issued: { "date-parts": [[2017, 8]] },
        abstract: `<jats:p>${ABSTRACT}</jats:p>`,
        "container-title": ["Journal"],
      }),
    ).toMatchObject({ doi: "10.1234/abc", year: 2017, abstract: ABSTRACT });
    expect(
      parseHalDoc({
        abstract_s: [ABSTRACT],
        fileMain_s: "https://hal.example/f.pdf",
        openAccess_bool: true,
      }),
    ).toEqual({
      abstract: ABSTRACT,
      pdf: "https://hal.example/f.pdf",
      open: true,
    });
  });
});

const alex = (extra: Record<string, unknown> = {}) => ({
  id: "https://openalex.org/W2614679811",
  doi: "https://doi.org/10.1016/j.envsoft.2017.05.002",
  title: "The suite of lumped GR hydrological models in an R package",
  publication_year: 2017,
  cited_by_count: 364,
  authorships: [{ author: { display_name: "L. Coron" } }],
  primary_location: {
    landing_page_url: "https://doi.org/10.1016/j.envsoft.2017.05.002",
  },
  best_oa_location: null,
  open_access: { is_oa: false },
  abstract_inverted_index: null,
  locations: [],
  ...extra,
});

describe("resolvePaper", () => {
  it("finds the abstract in HAL when the publisher withheld it everywhere else", async () => {
    // This is the paper from the bug report: no abstract in OpenAlex or
    // Crossref, no open PDF, but its HAL record has the abstract.
    const api = deps({
      "https://api.openalex.org/works/doi:": alex(),
      "https://api.crossref.org/works/": {
        message: { DOI: "10.1016/j.envsoft.2017.05.002", title: ["The suite"] },
      },
      "https://api.archives-ouvertes.fr/search/": {
        response: {
          docs: [{ abstract_s: [ABSTRACT], openAccess_bool: false }],
        },
      },
      "https://api.semanticscholar.org/": new Error("429"),
    });
    const paper = await resolvePaper(
      { doi: "10.1016/j.envsoft.2017.05.002" },
      api,
    );
    expect(paper?.abstract).toBe(ABSTRACT);
    expect(paper?.abstractFrom).toBe("HAL");
    expect(paper?.pdfUrls).toEqual([]);
    expect(paper?.access).toBe("closed");
  });

  it("collects every open PDF without losing the first", async () => {
    const api = deps({
      "https://api.openalex.org/works/doi:": alex({
        best_oa_location: { pdf_url: "https://a.example/p.pdf" },
        open_access: { is_oa: true },
      }),
      "https://api.crossref.org/": new Error("down"),
      "https://api.archives-ouvertes.fr/": {
        response: {
          docs: [
            { fileMain_s: "https://hal.example/p.pdf", openAccess_bool: true },
          ],
        },
      },
      "https://api.semanticscholar.org/": {
        abstract: ABSTRACT,
        openAccessPdf: { url: "https://s2.example/p.pdf" },
      },
    });
    const paper = await resolvePaper(
      { doi: "10.1016/j.envsoft.2017.05.002" },
      api,
    );
    expect(paper?.pdfUrls).toEqual([
      "https://a.example/p.pdf",
      "https://hal.example/p.pdf",
      "https://s2.example/p.pdf",
    ]);
    expect(paper?.abstractFrom).toBe("Semantic Scholar");
  });

  it("falls back to Crossref for the paper itself when OpenAlex is down", async () => {
    const api = deps({
      "https://api.openalex.org/": new Error("down"),
      "https://api.crossref.org/works/": {
        message: {
          DOI: "10.1234/x",
          title: ["From Crossref"],
          abstract: `<jats:p>${ABSTRACT}</jats:p>`,
        },
      },
      "https://api.archives-ouvertes.fr/": { response: { docs: [] } },
      "https://api.semanticscholar.org/": new Error("down"),
    });
    const paper = await resolvePaper({ doi: "10.1234/x" }, api);
    expect(paper?.title).toBe("From Crossref");
    expect(paper?.abstractFrom).toBe("Crossref");
  });

  it("gives up cleanly when nothing knows the paper", async () => {
    const api = deps({
      "https://api.openalex.org/": new Error("404"),
      "https://api.crossref.org/": new Error("404"),
    });
    expect(await resolvePaper({ doi: "10.1234/none" }, api)).toBeUndefined();
  });
});

describe("findPapers", () => {
  it("searches OpenAlex first", async () => {
    const api = deps({
      "https://api.openalex.org/works?": { results: [alex()] },
    });
    const found = await findPapers("lumped GR models", api, 3, 2015);
    expect(found.source).toBe("OpenAlex");
    expect(found.papers).toHaveLength(1);
    expect(api.calls[0]).toContain("from_publication_date:2015-01-01");
  });

  it("moves to Crossref, and says so, when OpenAlex rate-limits", async () => {
    const api = deps({
      "https://api.openalex.org/": {
        error: "Rate limit exceeded",
        retryAfter: 32,
      },
      "https://api.crossref.org/works?": {
        message: { items: [{ DOI: "10.1234/x", title: ["Hit"] }] },
      },
    });
    const found = await findPapers("anything", api);
    expect(found.source).toBe("Crossref");
    expect(found.note).toContain("OpenAlex was not available");
    expect(found.papers[0].title).toBe("Hit");
  });

  it("also asks Semantic Scholar and arXiv, in turns, once per paper", async () => {
    const api = deps({
      "https://api.openalex.org/works?": { results: [alex()] },
      "https://api.semanticscholar.org/graph/v1/paper/search?": {
        data: [
          {
            paperId: "s1",
            title: "Settling ponds for mine water",
            authors: [{ name: "J. Lee" }],
            year: 2021,
            citationCount: 12,
            externalIds: { DOI: "10.1016/j.mineng.2021.1" },
            openAccessPdf: { url: "https://x.org/ponds.pdf" },
          },
          // The same paper OpenAlex found, by title: not listed twice.
          { paperId: "s2", title: alex().title, externalIds: {} },
        ],
      },
      "https://export.arxiv.org/api/query?": `<?xml version="1.0"?><feed>
        <entry>
          <id>http://arxiv.org/abs/2403.01234v2</id>
          <published>2024-03-02T00:00:00Z</published>
          <title>Sediment &amp; flocculant
            dosing in tailings ponds</title>
          <summary>We model the settling.</summary>
          <author><name>A. Gómez</name></author>
          <author><name>B. Ruiz</name></author>
        </entry></feed>`,
    });
    const found = await findPapers("settling ponds", api, 6);
    expect(found.source).toBe("OpenAlex, Semantic Scholar, arXiv");
    expect(found.papers.map((paper) => paper.title)).toEqual([
      alex().title,
      "Settling ponds for mine water",
      "Sediment & flocculant dosing in tailings ponds",
    ]);
    expect(found.papers[1]).toMatchObject({
      doi: "10.1016/j.mineng.2021.1",
      citedBy: 12,
      access: "open",
      pdfUrls: ["https://x.org/ponds.pdf"],
    });
    // An arXiv paper reads by its arXiv DOI, like any other.
    expect(found.papers[2]).toMatchObject({
      doi: "10.48550/arxiv.2403.01234",
      authors: ["A. Gómez", "B. Ruiz"],
      year: 2024,
      venue: "arXiv",
      abstractFrom: "arXiv",
      pdfUrls: ["https://arxiv.org/pdf/2403.01234"],
    });
    expect(api.calls.some((url) => url.includes("search_query=all:settling+AND+all:ponds"))).toBe(true);
  });

  it("keeps OpenAlex's answer when the other two fail", async () => {
    const api = deps({
      "https://api.openalex.org/works?": { results: [alex()] },
      "https://api.semanticscholar.org/": new Error("429"),
      "https://export.arxiv.org/": new Error("timeout"),
    });
    const found = await findPapers("lumped GR models", api, 3);
    expect(found.source).toBe("OpenAlex");
    expect(found.papers).toHaveLength(1);
  });
});

describe("reading the text", () => {
  const text = {
    pages: [
      "Introduction\n\nLumped hydrological models transform precipitation into discharge at catchment scale and are widely used.",
      "Calibration of the GR4J model uses a Michel algorithm to minimise the error between simulated and observed flows.\n\nPlotting functions help automate tests of the results over catchment sets.",
      "References\n\nPerrin, C. et al. 2003. Improvement of a parsimonious model for streamflow simulation. Journal of Hydrology.",
    ],
  };

  it("picks the passages that answer a question, in reading order", () => {
    const found = bestPassages(text, "how is the GR4J model calibrated");
    const best = found.find((passage) => passage.text.includes("Michel"));
    expect(best?.page).toBe(2);
    // The references page has nothing to do with the question.
    expect(found.some((passage) => passage.page === 3)).toBe(false);
  });

  it("reads page ranges within the paper", () => {
    expect(parsePages("1,3-9", 3)).toEqual([1, 3]);
    expect(parsePages("0, x", 3)).toEqual([]);
  });

  it("cuts a long read and says where", () => {
    const read = excerpt(text, { maxChars: 100 });
    expect(read.truncated).toBe(true);
    expect(read.text).toHaveLength(100);
    expect(read.covers).toContain("cut at 100");
    expect(excerpt(text, { pages: "2" }).text).toContain("[page 2]");
    expect(excerpt(text, { query: "zebra" }).covers).toContain(
      "no passage matches",
    );
  });
});

describe("outline, figures and sections", () => {
  const text = {
    pages: [
      "Abstract\nWe present a thing.\n1 Introduction\nSome words about the thing and why it matters to everyone here.",
      "2 Methods\nWe collected data from forty catchments in France over ten years.\n2.1 Calibration\nThe GR4J model was calibrated with a Michel algorithm.\nFigure 1: Map of the forty catchments used in the study.\n3 Results\nThe calibrated model reached a median efficiency of 0.8.",
      "Table 2. Efficiency per catchment, sorted by size and by region of France.\n4 Conclusion\nIt works.\nReferences\n[1] Perrin 2003.",
    ],
  };

  it("lists the headings with their level and page, and skips captions and numbers", () => {
    expect(outline(text).map((e) => [e.title, e.level, e.page])).toEqual([
      ["Abstract", 1, 1],
      ["1 Introduction", 1, 1],
      ["2 Methods", 1, 2],
      ["2.1 Calibration", 2, 2],
      ["3 Results", 1, 2],
      ["4 Conclusion", 1, 3],
      ["References", 1, 3],
    ]);
    expect(
      outline({ pages: ["Table 3 Efficiency by region of France\n12 34"] }),
    ).toEqual([]);
  });

  it("lists figures and tables by caption, once each", () => {
    expect(figures(text)).toEqual([
      {
        label: "Figure 1",
        page: 2,
        caption: "Map of the forty catchments used in the study.",
      },
      {
        label: "Table 2",
        page: 3,
        caption:
          "Efficiency per catchment, sorted by size and by region of France.",
      },
    ]);
  });

  it("reads a section up to the next one of its level, subsections included", () => {
    const methods = section(text, "methods");
    expect(methods?.title).toBe("2 Methods");
    expect(methods?.text).toContain("forty catchments in France");
    expect(methods?.text).toContain("2.1 Calibration");
    expect(methods?.text).not.toContain("3 Results");
    expect(section(text, "calibration")?.text).not.toContain("3 Results");
  });

  it("finds a section by its usual other name, and says when there is none", () => {
    expect(section(text, "methodology")?.title).toBe("2 Methods");
    expect(section(text, "findings")?.title).toBe("3 Results");
    expect(section(text, "ethics")).toBeUndefined();
  });

  it("cuts a long section", () => {
    const long = section(text, "methods", 40);
    expect(long?.truncated).toBe(true);
    expect(long?.text).toHaveLength(40);
  });
});

describe("recognising a PDF the user gave", () => {
  const first = [
    "Journal of Hydrology: Regional Studies 46 (2023) 101381",
    "Contents lists available at ScienceDirect",
    "Construction of a daily streamflow dataset for Peru using a similarity-based regionalization approach",
    "Llauca, H., Lavado-Casimiro, W.",
    "https://doi.org/10.1016/J.EJRH.2023.101381.",
    "Abstract",
    "A total of 11,913 sub-catchments in Peru and transboundary catchments with neighboring countries",
    "in South America. This paper aims to develop a national hydrological model using physiographic data.",
    "Keywords: Peru, streamflow",
    "1. Introduction",
    "Water matters.",
  ].join("\n");

  it("finds the paper's own DOI in the first pages, not in its references", () => {
    expect(detectDoi({ pages: [first] })).toBe("10.1016/j.ejrh.2023.101381");
    expect(
      detectDoi({
        pages: ["no id here", "", "", "10.9999/cited.in.references.1"],
      }),
    ).toBeUndefined();
    expect(detectDoi({ pages: ["see (10.1234/abc.def)"] })).toBe(
      "10.1234/abc.def",
    );
  });

  it("guesses the title from the opening, skipping the journal's header", () => {
    expect(guessTitle({ pages: [first] })).toBe(
      "Construction of a daily streamflow dataset for Peru using a similarity-based regionalization approach",
    );
    expect(guessTitle({ pages: ["12\nhttps://example.org"] })).toBeUndefined();
  });

  it("reads the abstract up to the keywords", () => {
    const abstract = guessAbstract({ pages: [first] });
    expect(abstract).toContain("A total of 11,913 sub-catchments");
    expect(abstract).toContain("national hydrological model");
    expect(abstract).not.toContain("Keywords");
    expect(abstract).not.toContain("Water matters");
    expect(
      guessAbstract({ pages: ["Introduction\nNothing labelled."] }),
    ).toBeUndefined();
  });
});

describe("finding the real PDF", () => {
  const LLAUCA = {
    id: "https://openalex.org/W4",
    doi: "https://doi.org/10.1016/j.ejrh.2023.101381",
    title: "Construction of a daily streamflow dataset for Peru",
    publication_year: 2023,
    authorships: [{ author: { display_name: "Harold Llauca" } }],
    primary_location: {
      landing_page_url: "https://doi.org/10.1016/j.ejrh.2023.101381",
    },
    open_access: { is_oa: true, oa_status: "gold" },
    // What OpenAlex really returned: the article's graphical abstract, a JPG.
    best_oa_location: {
      is_oa: true,
      landing_page_url: "https://doi.org/10.1016/j.ejrh.2023.101381",
      pdf_url:
        "https://ars.els-cdn.com/content/image/1-s2.0-S221458182300068X-ga1_lrg.jpg",
    },
    locations: [
      {
        is_oa: true,
        landing_page_url: "https://doaj.org/article/7e0e",
        pdf_url: null,
      },
      {
        is_oa: true,
        landing_page_url: "https://hdl.handle.net/20.500.12542/2894",
        pdf_url: null,
      },
      {
        is_oa: false,
        landing_page_url: "https://paywalled.example/x",
        pdf_url: null,
      },
    ],
  };

  it("does not take a figure for the PDF, and tries the pages of open locations", () => {
    const paper = parseOpenAlexWork(LLAUCA)!;
    expect(paper.pdfUrls).toEqual([
      "https://doi.org/10.1016/j.ejrh.2023.101381",
      "https://doaj.org/article/7e0e",
      "https://hdl.handle.net/20.500.12542/2894",
    ]);
    expect(paper.access).toBe("open");
  });

  it("never tries a paywalled location of a closed paper", () => {
    const paper = parseOpenAlexWork({
      ...LLAUCA,
      open_access: { is_oa: false },
      best_oa_location: null,
      locations: [
        {
          is_oa: false,
          landing_page_url: "https://hal.example/x",
          pdf_url: null,
        },
      ],
    })!;
    expect(paper.pdfUrls).toEqual([]);
    expect(paper.access).toBe("closed");
  });

  it("recognises what cannot be a PDF", () => {
    expect(isLikelyPdfUrl("https://x.org/a/b.jpg?x=1")).toBe(false);
    expect(isLikelyPdfUrl("https://x.org/data.CSV")).toBe(false);
    expect(isLikelyPdfUrl("https://x.org/article/123")).toBe(true);
    expect(isLikelyPdfUrl("https://x.org/p.pdf")).toBe(true);
    expect(isLikelyPdfUrl("ftp://x.org/p.pdf")).toBe(false);
  });

  it("takes Europe PMC's open full text and its abstract", () => {
    expect(
      parseEuropePmc({
        resultList: {
          result: [
            {
              pmcid: "PMC123456",
              isOpenAccess: "Y",
              abstractText: `<p>${ABSTRACT}</p>`,
            },
          ],
        },
      }),
    ).toEqual({
      abstract: ABSTRACT,
      pdf: "https://europepmc.org/backend/ptpmcrender.fcgi?accid=PMC123456&blobtype=pdf",
      open: true,
    });
    expect(
      parseEuropePmc({
        resultList: { result: [{ pmcid: "PMC1", isOpenAccess: "N" }] },
      }),
    ).toEqual({});
    expect(parseEuropePmc({ resultList: { result: [] } })).toEqual({});
  });

  it("asks Europe PMC only when nothing else holds a PDF, and ignores a JPG from Semantic Scholar", async () => {
    const api = deps({
      "https://api.openalex.org/works/doi:": alex({
        open_access: { is_oa: false },
      }),
      "https://api.crossref.org/": new Error("down"),
      "https://api.archives-ouvertes.fr/": { response: { docs: [] } },
      "https://api.semanticscholar.org/": {
        openAccessPdf: { url: "https://x.org/fig.jpg" },
      },
      "https://www.ebi.ac.uk/europepmc/": {
        resultList: { result: [{ pmcid: "PMC9", isOpenAccess: "Y" }] },
      },
    });
    const paper = await resolvePaper(
      { doi: "10.1016/j.envsoft.2017.05.002" },
      api,
    );
    expect(paper?.pdfUrls).toEqual([
      "https://europepmc.org/backend/ptpmcrender.fcgi?accid=PMC9&blobtype=pdf",
    ]);
    expect(paper?.access).toBe("open");
  });

  it("names a PDF kept from a file the user gave", () => {
    expect(parseRef("pdf:0123456789abcdef")).toEqual({
      cached: "pdf:0123456789abcdef",
    });
    expect(parseRef("PDF:0123456789ABCDEF")).toEqual({
      cached: "pdf:0123456789abcdef",
    });
  });
});
