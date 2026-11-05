// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./paperStore", () => ({ paperKnown: vi.fn() }));

import {
  citedLabel,
  paperCitations,
  paperPassages,
  recordPaperPassage,
  savedLocation,
} from "./paperCitations";

const DOI = "https://doi.org/10.1016/j.envsoft.2017.05.002";
const KEY = "10.1016/j.envsoft.2017.05.002";
const A = "The package allows users to calibrate and run hourly models.";
const B = "While the core of the models is implemented in Fortran.";

beforeEach(() => localStorage.clear());

describe("paper passages", () => {
  it("keeps what each answer cited, once, in the order it was cited", () => {
    recordPaperPassage(DOI, A);
    recordPaperPassage(DOI, B);
    recordPaperPassage(DOI, A);
    expect(paperPassages(KEY)).toEqual([A, B]);
  });

  it("ignores web pages, empty quotes and fragments too short to find", () => {
    recordPaperPassage("https://example.com/post", A);
    recordPaperPassage(DOI, undefined);
    recordPaperPassage(DOI, "  too short ");
    expect(localStorage.getItem("monocode.paperPassages")).toBeNull();
    expect(paperPassages(KEY)).toEqual([]);
  });

  it("files an arXiv link under the same key the cache uses", () => {
    recordPaperPassage("https://arxiv.org/abs/1706.03762v5", A);
    expect(paperPassages("10.48550/arxiv.1706.03762")).toEqual([A]);
  });

  it("keeps the latest passages of a paper and the latest papers", () => {
    for (let index = 0; index < 45; index += 1)
      recordPaperPassage(
        DOI,
        `Passage number ${index} says something specific.`,
      );
    const kept = paperPassages(KEY);
    expect(kept).toHaveLength(40);
    expect(kept[0]).toContain("number 5 ");
    for (let index = 0; index < 62; index += 1)
      recordPaperPassage(`https://doi.org/10.1234/paper${index}`, A);
    expect(paperPassages("10.1234/paper0")).toEqual([]);
    expect(paperPassages("10.1234/paper61")).toEqual([A]);
  });

  it("survives a broken store", () => {
    localStorage.setItem("monocode.paperPassages", "{not json");
    expect(paperPassages(KEY)).toEqual([]);
    recordPaperPassage(DOI, A);
    expect(paperPassages(KEY)).toEqual([A]);
  });
});

describe("citation numbers", () => {
  it("keeps the number an answer gave each citation", () => {
    recordPaperPassage(DOI, A, 1);
    recordPaperPassage(DOI, B, 3);
    expect(paperCitations(KEY)).toEqual([
      { passage: A, number: 1 },
      { passage: B, number: 3 },
    ]);
    expect(paperPassages(KEY)).toEqual([A, B]);
  });

  it("follows a later answer that numbered the same passage differently", () => {
    recordPaperPassage(DOI, A, 1);
    recordPaperPassage(DOI, A, 2);
    expect(paperCitations(KEY)).toEqual([{ passage: A, number: 2 }]);
    // Shown again with the same number: nothing is written.
    const before = localStorage.getItem("monocode.paperPassages");
    recordPaperPassage(DOI, A, 2);
    expect(localStorage.getItem("monocode.paperPassages")).toBe(before);
  });

  it("fills in a number for a passage first saved without one", () => {
    recordPaperPassage(DOI, A);
    expect(paperCitations(KEY)).toEqual([{ passage: A }]);
    recordPaperPassage(DOI, A, 4);
    expect(paperCitations(KEY)).toEqual([{ passage: A, number: 4 }]);
  });

  it("still reads passages saved as plain text before numbers were kept", () => {
    localStorage.setItem(
      "monocode.paperPassages",
      JSON.stringify({ [KEY]: [A] }),
    );
    expect(paperCitations(KEY)).toEqual([{ passage: A }]);
    recordPaperPassage(DOI, B, 2);
    expect(paperCitations(KEY)).toEqual([
      { passage: A },
      { passage: B, number: 2 },
    ]);
  });

  it("names a passage by its citation and page", () => {
    expect(citedLabel(1, 3)).toBe("citation 1, page 3");
    expect(citedLabel(undefined, 3)).toBe("page 3");
    expect(citedLabel(2)).toBe("citation 2");
    expect(citedLabel()).toBe("");
  });
});

describe("savedLocation", () => {
  it("names the folder in words for a macOS path and a Windows path", () => {
    expect(
      savedLocation(
        "/Users/me/Documents/MonoBOT/Research/Coron et al. (2017) - The suite.pdf",
      ),
    ).toEqual({
      folder: "Documents / MonoBOT / Research",
      file: "Coron et al. (2017) - The suite.pdf",
    });
    expect(
      savedLocation("C:\\Users\\me\\Documents\\MonoBOT\\Banana\\Paper.pdf"),
    ).toEqual({ folder: "Documents / MonoBOT / Banana", file: "Paper.pdf" });
  });

  it("shows the last two folders when the copy is not in a Documents folder", () => {
    expect(savedLocation("/data/papers/Research/x.pdf")).toEqual({
      folder: "papers / Research",
      file: "x.pdf",
    });
  });
});
