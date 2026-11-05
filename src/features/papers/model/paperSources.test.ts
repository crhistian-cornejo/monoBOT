// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  normalizeSourceUrl,
  paperKeyOfSource,
  rememberPaperSource,
} from "./paperSources";
import { paperKeyOf } from "./openPaper";

describe("normalizeSourceUrl", () => {
  it("reads the same address however it was written", () => {
    expect(normalizeSourceUrl("https://WWW.Example.org/files/guide.pdf#page=4")).toBe(
      "www.example.org/files/guide.pdf",
    );
    expect(normalizeSourceUrl("http://www.example.org/files/guide.pdf/")).toBe(
      "www.example.org/files/guide.pdf",
    );
  });

  it("keeps the query, which can name the file", () => {
    expect(normalizeSourceUrl("https://example.org/get?id=12")).toBe(
      "example.org/get?id=12",
    );
  });

  it("ignores what is not a web address", () => {
    expect(normalizeSourceUrl("file:///C:/guide.pdf")).toBeUndefined();
    expect(normalizeSourceUrl("not a link")).toBeUndefined();
  });
});

describe("paper sources", () => {
  beforeEach(() => localStorage.clear());

  it("maps every link a PDF came from back to it", () => {
    rememberPaperSource(
      ["https://example.org/report", "https://cdn.example.org/report.pdf"],
      "pdf:0011223344556677",
    );
    expect(paperKeyOfSource("https://example.org/report#summary")).toBe(
      "pdf:0011223344556677",
    );
    expect(paperKeyOf("https://cdn.example.org/report.pdf")).toBe(
      "pdf:0011223344556677",
    );
  });

  it("leaves papers with a DOI to their DOI", () => {
    rememberPaperSource(["https://example.org/x.pdf"], "10.1/abc");
    expect(paperKeyOfSource("https://example.org/x.pdf")).toBeUndefined();
    expect(paperKeyOf("https://doi.org/10.5194/gmd-15-7017-2022")).toBe(
      "10.5194/gmd-15-7017-2022",
    );
  });

  it("knows nothing of a link it was never given", () => {
    expect(paperKeyOf("https://example.org/elsewhere")).toBeUndefined();
  });
});
