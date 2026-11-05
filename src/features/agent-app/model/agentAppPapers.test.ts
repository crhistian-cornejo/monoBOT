import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import type { LoadedPaper } from "../../papers/model/paperStore";
import type { Paper } from "../../papers/model/papers";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

const paper: Paper = {
  key: "10.1016/j.envsoft.2017.05.002",
  doi: "10.1016/j.envsoft.2017.05.002",
  title: "The suite of lumped GR hydrological models in an R package",
  authors: ["L. Coron", "G. Thirel", "O. Delaigue"],
  year: 2017,
  venue: "Environmental Modelling & Software",
  citedBy: 364,
  abstract:
    "Lumped hydrological models are catchment-scale representations of the transformation of precipitation into discharge.",
  abstractFrom: "HAL",
  pdfUrls: [],
  access: "closed",
};

function fixture(loaded?: LoadedPaper) {
  const source = newSession("codex", "/tmp/project", "codex:test");
  source.id = "research";
  const host = {
    isMono: () => true,
    papers: {
      find: vi.fn(async () => ({ papers: [paper], source: "OpenAlex" })),
      load: vi.fn(async () => loaded),
      addPdf: vi.fn(async () => loaded as LoadedPaper),
      renderPage: vi.fn(async (_key: string, page: number) => ({
        path: `/cache/papers/images/abc-p${page}.png`,
        page,
        pages: 5,
      })),
    },
    saveArtifact: vi.fn(async (input) => ({
      ...input,
      createdAt: 1,
      updatedAt: 1,
    })),
    postArtifact: vi.fn(),
  } as unknown as AgentAppHost;
  return { source, host };
}

describe("papers.find", () => {
  it("lists candidates with a ref the agent can read, not the whole abstract", async () => {
    const { source, host } = fixture();
    const result = (await handleAgentApp(
      source,
      "find-1",
      "papers.find",
      { query: "lumped GR models in R", limit: 3, fromYear: 2015 },
      host,
    )) as { papers: Record<string, unknown>[]; source: string };
    expect(host.papers!.find).toHaveBeenCalledWith(
      "lumped GR models in R",
      3,
      2015,
    );
    expect(result.source).toBe("OpenAlex");
    expect(result.papers[0]).toMatchObject({
      ref: "10.1016/j.envsoft.2017.05.002",
      label: "Coron et al. (2017)",
      access: "closed",
    });
    expect(String(result.papers[0].abstractStart).length).toBeLessThan(260);
  });

  it("rejects a bad limit, a bad year and an unknown field", async () => {
    const { source, host } = fixture();
    const call = (input: Record<string, unknown>) =>
      handleAgentApp(source, "find-2", "papers.find", input, host);
    await expect(call({ query: "x", limit: 50 })).rejects.toThrow("limit");
    await expect(call({ query: "x", fromYear: 15 })).rejects.toThrow(
      "fromYear",
    );
    await expect(call({ query: "x", download: true })).rejects.toThrow(
      "Unknown",
    );
    await expect(call({})).rejects.toThrow("query");
  });
});

describe("papers.read", () => {
  it("tells the agent a closed paper is abstract-only, and files it in Documents", async () => {
    const { source, host } = fixture({
      paper,
      availability: "abstract-only",
      reason: "It is not open access and no repository holds a free copy.",
      cached: false,
    });
    const result = (await handleAgentApp(
      source,
      "read-1",
      "papers.read",
      { ref: "https://doi.org/10.1016/j.envsoft.2017.05.002" },
      host,
    )) as Record<string, unknown>;
    expect(host.papers!.load).toHaveBeenCalledWith(
      { doi: "10.1016/j.envsoft.2017.05.002" },
      { agent: undefined },
    );
    expect(result).toMatchObject({
      availability: "abstract-only",
      abstractFrom: "HAL",
      why: expect.stringContaining("not open access"),
      note: expect.stringContaining("full text was not accessible"),
    });
    expect(result).not.toHaveProperty("fullText");
    expect(host.saveArtifact).toHaveBeenCalledWith(
      expect.objectContaining({
        id: "paper-10-1016-j-envsoft-2017-05-002",
        kind: "document",
        body: expect.stringContaining("Abstract only."),
      }),
    );
    // The reference line is how the Documents page finds the cached paper.
    expect(vi.mocked(host.saveArtifact!).mock.calls[0][0].body).toContain(
      "[monobot-paper]: paper:10.1016%2Fj.envsoft.2017.05.002",
    );
    expect(host.postArtifact).toHaveBeenCalledWith(
      "research",
      expect.objectContaining({ id: "paper-10-1016-j-envsoft-2017-05-002" }),
    );
  });

  it("posts a paper's card once per conversation, however often it is read", async () => {
    const { source, host } = fixture({
      paper,
      availability: "abstract-only",
      cached: false,
    });
    source.id = "research-twice";
    for (const request of ["a", "b", "c"])
      await handleAgentApp(
        source,
        request,
        "papers.read",
        { ref: paper.key },
        host,
      );
    expect(host.saveArtifact).toHaveBeenCalledTimes(3);
    expect(host.postArtifact).toHaveBeenCalledTimes(1);
  });

  it("returns the passages that answer a question when the text was read", async () => {
    const { source, host } = fixture({
      paper: { ...paper, access: "open" },
      availability: "full-text",
      cached: true,
      text: {
        pages: [
          "Introduction\n\nLumped hydrological models transform precipitation into discharge at catchment scale and are widely used.",
          "Calibration of the GR4J model uses a Michel algorithm to minimise the error between simulated and observed flows.",
        ],
      },
    });
    const result = (await handleAgentApp(
      source,
      "read-2",
      "papers.read",
      { ref: paper.key, query: "how is GR4J calibrated" },
      host,
    )) as {
      fullText: { text: string; pageCount: number };
      availability: string;
    };
    expect(result.availability).toBe("full-text");
    expect(result.fullText.pageCount).toBe(2);
    expect(result.fullText.text).toContain("[page 2]");
    expect(result.fullText.text).toContain("Michel algorithm");
  });

  it("does not make up a paper nobody has", async () => {
    const { source, host } = fixture(undefined);
    await expect(
      handleAgentApp(
        source,
        "read-3",
        "papers.read",
        { ref: "no such paper" },
        host,
      ),
    ).rejects.toThrow("No paper was found");
  });

  it("still reads the paper when the Documents entry cannot be saved", async () => {
    const { source, host } = fixture({
      paper,
      availability: "abstract-only",
      cached: false,
    });
    host.saveArtifact = vi.fn(async () => {
      throw new Error("disk full");
    });
    source.id = "research-disk";
    await expect(
      handleAgentApp(source, "read-4", "papers.read", { ref: paper.key }, host),
    ).resolves.toMatchObject({ availability: "abstract-only" });
  });
});

const FULL_TEXT = {
  pages: [
    "Abstract\nWe present a thing for catchments in France and beyond.",
    "2 Methods\nWe collected data from forty catchments in France over ten years.\nFigure 1: Map of the forty catchments used in the study.\n3 Results\nThe calibrated model reached a median efficiency of 0.8.",
  ],
};

describe("more of an open paper", () => {
  const open = () =>
    fixture({
      paper: { ...paper, access: "open" },
      availability: "full-text",
      cached: true,
      text: FULL_TEXT,
    });

  it("returns the outline and figures, and a named section on request", async () => {
    const { source, host } = open();
    const result = (await handleAgentApp(
      source,
      "sec-1",
      "papers.read",
      { ref: paper.key, section: "methods" },
      host,
    )) as {
      outline: string[];
      figures: string[];
      fullText: { covers: string; text: string };
    };
    expect(result.outline).toEqual([
      "Abstract (p. 1)",
      "2 Methods (p. 2)",
      "3 Results (p. 2)",
    ]);
    expect(result.figures).toEqual([
      "Figure 1 (p. 2): Map of the forty catchments used in the study.",
    ]);
    expect(result.fullText.covers).toContain('section "2 Methods"');
    expect(result.fullText.text).toContain("forty catchments in France");
    expect(result.fullText.text).not.toContain("median efficiency");
  });

  it("says which sections exist when the one asked for does not", async () => {
    const { source, host } = open();
    const result = (await handleAgentApp(
      source,
      "sec-2",
      "papers.read",
      { ref: paper.key, section: "ethics" },
      host,
    )) as { fullText: { covers: string; text: string } };
    expect(result.fullText.text).toBe("");
    expect(result.fullText.covers).toContain("pick one from the outline");
  });

  it("draws a page as an image the agent can put in its reply", async () => {
    const { source, host } = open();
    const result = (await handleAgentApp(
      source,
      "fig-1",
      "papers.figure",
      { ref: paper.key, page: 2 },
      host,
    )) as { markdown: string; onThisPage: string[]; path: string };
    expect(host.papers!.renderPage).toHaveBeenCalledWith(paper.key, 2);
    expect(result.markdown).toBe(
      "![Figure 1 of Coron et al. (2017)](</cache/papers/images/abc-p2.png>)",
    );
    expect(result.onThisPage[0]).toContain(
      "Figure 1: Map of the forty catchments",
    );
  });

  it("will not draw a paper that has no PDF here, or a bad page", async () => {
    const closed = fixture({
      paper,
      availability: "abstract-only",
      reason: "It is not open access and no repository holds a free copy.",
      cached: false,
    });
    await expect(
      handleAgentApp(
        closed.source,
        "fig-2",
        "papers.figure",
        { ref: paper.key, page: 1 },
        closed.host,
      ),
    ).rejects.toThrow("no PDF of this paper");
    const { source, host } = open();
    await expect(
      handleAgentApp(
        source,
        "fig-3",
        "papers.figure",
        { ref: paper.key, page: 0 },
        host,
      ),
    ).rejects.toThrow("page");
    await expect(
      handleAgentApp(
        source,
        "fig-4",
        "papers.figure",
        { ref: paper.key },
        host,
      ),
    ).rejects.toThrow("page");
  });
});

describe("a PDF the user gives", () => {
  const local = {
    paper: {
      key: "pdf:0123456789abcdef",
      title: "A report with no DOI",
      authors: [],
      pdfUrls: [],
      access: "unknown" as const,
    },
    availability: "full-text" as const,
    cached: true,
    text: FULL_TEXT,
  };

  it("registers it, hands back a ref to read it by, and files it in Documents", async () => {
    const { source, host } = fixture(local);
    const result = (await handleAgentApp(
      source,
      "add-1",
      "papers.add",
      { path: "/Users/me/Downloads/report.pdf", title: "A report with no DOI" },
      host,
    )) as { ref: string; availability: string; outline: string[] };
    expect(host.papers!.addPdf).toHaveBeenCalledWith(
      "/Users/me/Downloads/report.pdf",
      { title: "A report with no DOI", agent: undefined },
    );
    expect(result.ref).toBe("pdf:0123456789abcdef");
    expect(result.availability).toBe("full-text");
    expect(result.outline).toContain("2 Methods (p. 2)");
    expect(host.saveArtifact).toHaveBeenCalledWith(
      expect.objectContaining({ id: "paper-pdf-0123456789abcdef" }),
    );
  });

  it("is the copy of the paper it is told, by its ref", async () => {
    const { source, host } = fixture(local);
    await handleAgentApp(
      source,
      "add-2",
      "papers.add",
      { path: "/tmp/p.pdf", ref: "https://doi.org/10.1016/j.ejrh.2023.101381" },
      host,
    );
    expect(host.papers!.addPdf).toHaveBeenCalledWith("/tmp/p.pdf", {
      ref: { doi: "10.1016/j.ejrh.2023.101381" },
      agent: undefined,
    });
  });

  it("refuses a path that is not absolute, a missing path and an unknown field", async () => {
    const { source, host } = fixture(local);
    const call = (input: Record<string, unknown>) =>
      handleAgentApp(source, "add-3", "papers.add", input, host);
    await expect(call({ path: "report.pdf" })).rejects.toThrow("absolute path");
    await expect(call({})).rejects.toThrow("path");
    await expect(
      call({ path: "/tmp/p.pdf", url: "https://x" }),
    ).rejects.toThrow("Unknown");
    expect(host.papers!.addPdf).not.toHaveBeenCalled();
  });

  it("reads it again later by that ref, from the cache", async () => {
    const { source, host } = fixture(local);
    const result = (await handleAgentApp(
      source,
      "add-4",
      "papers.read",
      { ref: "pdf:0123456789abcdef", section: "methods" },
      host,
    )) as { fullText: { covers: string } };
    expect(host.papers!.load).toHaveBeenCalledWith(
      { cached: "pdf:0123456789abcdef" },
      { agent: undefined },
    );
    expect(result.fullText.covers).toContain('section "2 Methods"');
  });
});

describe("a site that blocks downloads", () => {
  it("tells the agent the paper may be free, that it was blocked, and what the user can do", async () => {
    const { source, host } = fixture({
      paper: { ...paper, access: "open" },
      availability: "abstract-only",
      cached: false,
      blocked: true,
      reason:
        "This paper is open access, but the publisher's site refuses automatic downloads (a human check is asked for), so MonoBOT cannot fetch the PDF. Download it from the publisher's page and add it with Add PDF.",
    });
    const result = (await handleAgentApp(
      source,
      "blocked-1",
      "papers.read",
      { ref: paper.key },
      host,
    )) as { blocked: boolean; why: string; note: string };
    expect(result.blocked).toBe(true);
    expect(result.why).toContain("refuses automatic downloads");
    expect(result.note).toContain("papers.add");
    expect(result.note).toContain("Add PDF");
  });
});

describe("whose folder a paper goes in", () => {
  it("hands the agent's name to the store, which keeps the PDF in that agent's folder of Documents", async () => {
    const { source, host } = fixture({
      paper: { ...paper, access: "open" },
      availability: "full-text",
      cached: true,
      text: FULL_TEXT,
    });
    host.monoOf = () => ({ id: "m1", projects: [], name: "Research" });
    await handleAgentApp(
      source,
      "own-1",
      "papers.read",
      { ref: paper.key },
      host,
    );
    await handleAgentApp(
      source,
      "own-2",
      "papers.add",
      { path: "/tmp/p.pdf" },
      host,
    );
    expect(host.papers!.load).toHaveBeenCalledWith(expect.anything(), {
      agent: "Research",
    });
    expect(host.papers!.addPdf).toHaveBeenCalledWith("/tmp/p.pdf", {
      agent: "Research",
    });
  });
});
