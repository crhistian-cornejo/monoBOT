import { describe, expect, it } from "vitest";
import { locateInItems, locatePage, rangeParts } from "./locate";

const PASSAGE =
  "The package allows users to calibrate and run hourly to annual models on catchment sets and to analyse their outputs.";

/** The text of a range, to compare with what was quoted. */
function read(items: string[], passage: string) {
  const found = locateInItems(items, passage);
  if (!found) return undefined;
  return {
    coverage: found.coverage,
    text: rangeParts(items, found)
      .map(({ item, from, to }) => items[item].slice(from, to))
      .join(""),
  };
}

describe("locateInItems", () => {
  it("finds a sentence split across the lines of a PDF", () => {
    const items = [
      "While the core of the models is implemented in Fortran. ",
      "The package allows users to calibrate and run hourly ",
      "to annual models on catchment sets and to ",
      "analyse their outputs. A number of options follow.",
    ];
    const hit = read(items, PASSAGE);
    expect(hit?.coverage).toBe(1);
    expect(hit?.text).toBe(
      "The package allows users to calibrate and run hourly to annual models on catchment sets and to analyse their outputs",
    );
  });

  it("ignores hyphenation, ligatures, quotes and case", () => {
    const items = [
      "The ef-",
      "ﬁciency of the “GR4J” model is high for catchments in France.",
    ];
    const quoted =
      'the efficiency of the "gr4j" model is HIGH for catchments in France.';
    expect(locateInItems(items, quoted)?.coverage).toBe(1);
  });

  it("finds the part that is there when the paraphrase drifts at the end", () => {
    const items = [
      "The package allows users to calibrate and run hourly to annual models on catchment sets.",
    ];
    const hit = locateInItems(items, `${PASSAGE} It is very good.`);
    expect(hit).toBeDefined();
    expect(hit!.coverage).toBeLessThan(1);
    expect(hit!.coverage).toBeGreaterThan(0.6);
  });

  it("bridges a sentence whose middle was reworded", () => {
    const items = [
      "The package allows users to calibrate and run models of many kinds on catchment sets and to analyse their outputs.",
    ];
    const hit = locateInItems(items, PASSAGE);
    expect(hit?.coverage).toBeGreaterThan(0.85);
  });

  it("does not invent a match for text that is not there, or is too short", () => {
    const items = ["Calibration of the GR4J model uses a Michel algorithm."];
    expect(
      locateInItems(
        items,
        "Deep learning dominates the benchmark leaderboards today.",
      ),
    ).toBeUndefined();
    expect(locateInItems(items, "the model")).toBeUndefined();
  });

  it("reports the item and offset where the match starts and ends", () => {
    const found = locateInItems(
      ["abc ", "Calibration of the GR4J model works. ", "end"],
      "Calibration of the GR4J model works",
    );
    expect(found).toMatchObject({ startItem: 1, startOffset: 0, endItem: 1 });
  });
});

describe("locatePage", () => {
  const pages = [
    "Introduction to hydrology and rainfall.",
    PASSAGE,
    "References",
  ];

  it("picks the page that holds the passage", () => {
    expect(locatePage(pages, PASSAGE)).toEqual({ page: 2, coverage: 1 });
  });

  it("is undefined when no page has it", () => {
    expect(
      locatePage(
        pages,
        "Transformers replace recurrence with attention everywhere.",
      ),
    ).toBeUndefined();
  });

  it("tries the page the agent named first", () => {
    const twice = [PASSAGE, "Other text.", PASSAGE];
    expect(locatePage(twice, PASSAGE, 3)?.page).toBe(3);
    expect(locatePage(twice, PASSAGE)?.page).toBe(1);
  });
});
