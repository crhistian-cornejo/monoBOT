import { describe, expect, it } from "vitest";
import { cleanPdfSelection, paperQuote, paperSourceLine } from "./paperQuote";

describe("cleanPdfSelection", () => {
  it("joins the lines of a selection into one", () => {
    expect(cleanPdfSelection("Hydrologic models are used\nfor numerous  applications")).toBe(
      "Hydrologic models are used for numerous applications",
    );
  });

  it("mends a word the layout broke across lines", () => {
    expect(cleanPdfSelection("the increas-\ning complexity")).toBe(
      "the increasing complexity",
    );
  });

  it("keeps a hyphen that belongs to the word", () => {
    expect(cleanPdfSelection("rainfall-runoff and semi-\nDistributed")).toBe(
      "rainfall-runoff and semi- Distributed",
    );
  });
});

describe("paperQuote", () => {
  const source = {
    label: "Chlumsky et al. (2022)",
    title: "RavenR v2.1.4",
    page: 3,
    link: "https://doi.org/10.5194/gmd-15-7017-2022",
  };

  it("names the paper, the page and its link after the words", () => {
    expect(paperQuote("flexible\nmodels", source)).toBe(
      "flexible models\n\n— Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3 (https://doi.org/10.5194/gmd-15-7017-2022)",
    );
  });

  it("leaves out what it does not know", () => {
    expect(paperSourceLine({ title: "A guide" })).toBe("— “A guide”");
  });

  it("gives nothing for an empty selection", () => {
    expect(paperQuote("  \n ", source)).toBe("");
  });
});
