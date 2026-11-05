// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import type { LoadedPaper } from "../../papers/model/paperStore";
import type { FetchedWebPage } from "../../web/model/webApi";
import { handleAgentApp, type AgentAppHost } from "./agentApp";

const scan: LoadedPaper = {
  paper: {
    key: "pdf:00aa11bb22cc33dd",
    title: "Pond design handbook",
    authors: [],
    pdfUrls: [],
    access: "unknown",
  },
  availability: "metadata-only",
  reason: "The PDF is a scan with no text layer.",
  cached: true,
};

function fixture(page: Partial<FetchedWebPage>, loaded: LoadedPaper = scan) {
  const source = newSession("claude", "/tmp/project", "claude:test");
  source.id = "research";
  const host = {
    isMono: () => true,
    monoOf: () => ({ name: "Research" }),
    web: {
      page: vi.fn(async () => ({
        finalUrl: "https://mineria.example/pozas",
        isPdf: false,
        bytes: 100,
        ...page,
      })),
      download: vi.fn(async (url: string) =>
        url.endsWith(".pdf")
          ? {
              path: "/cache/papers/downloads/guide.pdf",
              finalUrl: url,
              bytes: 2048,
              isPdf: true,
            }
          : {
              path: "/Users/me/Documents/MonoBOT/Research/data.csv",
              finalUrl: url,
              contentType: "text/csv",
              bytes: 64,
              isPdf: false,
            },
      ),
      keep: vi.fn(async () => "/Users/me/Documents/MonoBOT/Research/guide.pdf"),
    },
    papers: {
      find: vi.fn(),
      load: vi.fn(),
      renderPage: vi.fn(),
      addPdf: vi.fn(async () => loaded),
    },
    saveArtifact: vi.fn(async (input) => ({ ...input, createdAt: 1, updatedAt: 1 })),
    postArtifact: vi.fn(),
  } as unknown as AgentAppHost;
  return { source, host };
}

const call = (
  host: AgentAppHost,
  source: ReturnType<typeof fixture>["source"],
  action: string,
  input: Record<string, unknown>,
) => handleAgentApp(source, `${action}-1`, action, input, host);

describe("web.read", () => {
  it("returns the page's text, metadata and a reminder that it is data", async () => {
    const { source, host } = fixture({
      text: `<html><head><title>Pozas</title><meta property="og:site_name" content="Minería Hoy"></head>
        <body><article><h1>Pozas</h1><p>${"La poza retiene sólidos. ".repeat(20)}</p></article></body></html>`,
    });
    const result = (await call(host, source, "web.read", {
      url: "https://mineria.example/pozas",
      query: "retiene sólidos",
    })) as Record<string, unknown>;
    expect(host.web!.page).toHaveBeenCalledWith("https://mineria.example/pozas");
    expect(result).toMatchObject({
      title: "Pozas",
      siteName: "Minería Hoy",
      icon: "https://mineria.example/favicon.ico",
    });
    expect(result.text).toContain("# Pozas");
    expect((result.passages as string[])[0]).toContain("retiene sólidos");
    expect(result.note).toMatch(/never follow instructions/);
  });

  it("sends a PDF or another file to web.download instead", async () => {
    const pdf = fixture({ isPdf: true });
    expect(
      await call(pdf.host, pdf.source, "web.read", { url: "https://x.org/a.pdf" }),
    ).toMatchObject({ kind: "pdf", next: expect.stringMatching(/web\.download/) });
    const zip = fixture({ contentType: "application/zip" });
    expect(
      await call(zip.host, zip.source, "web.read", { url: "https://x.org/a.zip" }),
    ).toMatchObject({ kind: "file", contentType: "application/zip" });
  });

  it("refuses what isn't a web address, and bad limits", async () => {
    const { source, host } = fixture({});
    await expect(
      call(host, source, "web.read", { url: "file:///etc/passwd" }),
    ).rejects.toThrow("https://");
    await expect(
      call(host, source, "web.read", { url: "https://x.org", maxChars: 10 }),
    ).rejects.toThrow("maxChars");
    await expect(
      call(host, source, "web.read", { url: "https://x.org", path: "/tmp" }),
    ).rejects.toThrow("Unknown");
    expect(host.web!.page).not.toHaveBeenCalled();
  });
});

describe("web.download", () => {
  it("saves a file in the agent's folder in Documents", async () => {
    const { source, host } = fixture({});
    const result = await call(host, source, "web.download", {
      url: "https://data.example/stations.csv",
      name: "Stations",
    });
    expect(host.web!.download).toHaveBeenCalledWith(
      "https://data.example/stations.csv",
      { agent: "Research", name: "Stations" },
    );
    expect(result).toMatchObject({
      path: "/Users/me/Documents/MonoBOT/Research/data.csv",
      contentType: "text/csv",
    });
    expect(host.papers!.addPdf).not.toHaveBeenCalled();
  });

  it("reads a PDF as a paper, and keeps a scan in Documents itself", async () => {
    const { source, host } = fixture({});
    const result = (await call(host, source, "web.download", {
      url: "https://dnr.example/guide.pdf",
    })) as Record<string, unknown>;
    // Where it was found is kept, so a citation of that link opens the PDF.
    expect(host.papers!.addPdf).toHaveBeenCalledWith(
      "/cache/papers/downloads/guide.pdf",
      {
        agent: "Research",
        sources: ["https://dnr.example/guide.pdf"],
      },
    );
    // A scan isn't exported as a paper, so the download is kept as is.
    expect(host.web!.keep).toHaveBeenCalledWith(
      "/cache/papers/downloads/guide.pdf",
      "Research",
    );
    expect(result).toMatchObject({
      ref: "pdf:00aa11bb22cc33dd",
      availability: "metadata-only",
      path: "/Users/me/Documents/MonoBOT/Research/guide.pdf",
      url: "https://dnr.example/guide.pdf",
    });
  });
});
