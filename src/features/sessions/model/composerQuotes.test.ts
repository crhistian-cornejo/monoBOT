import { describe, expect, it } from "vitest";
import {
  addComposerQuote,
  quoteChipLabel,
  quoteChipSource,
  splitQuoteSource,
  withComposerQuotes,
} from "./composerQuotes";

const fromPaper =
  "Flexible models need scripting.\n\n— Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3 (https://doi.org/10.5194/gmd-15-7017-2022)";

describe("splitQuoteSource", () => {
  it("separates the words from the line that says where they are from", () => {
    expect(splitQuoteSource(fromPaper)).toEqual({
      body: "Flexible models need scripting.",
      source:
        "— Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3 (https://doi.org/10.5194/gmd-15-7017-2022)",
    });
  });

  it("leaves a quote from the chat whole", () => {
    expect(splitQuoteSource("Line one\nline two")).toEqual({
      body: "Line one\nline two",
    });
  });
});

describe("chip text", () => {
  it("shows the start of the quote on one line", () => {
    expect(quoteChipLabel("a\nb   c")).toBe("a b c");
    expect(quoteChipLabel("x".repeat(80), 10)).toBe(`${"x".repeat(9)}…`);
  });

  it("names the paper and page without the link", () => {
    expect(quoteChipSource(fromPaper)).toBe(
      "Chlumsky et al. (2022), “RavenR v2.1.4”, p. 3",
    );
    expect(quoteChipSource("plain")).toBeUndefined();
  });
});

describe("addComposerQuote", () => {
  it("adds each quote once", () => {
    const one = addComposerQuote([], " words ", "a");
    expect(one).toEqual([{ id: "a", text: "words" }]);
    expect(addComposerQuote(one, "words", "b")).toEqual(one);
    expect(addComposerQuote(one, "  ", "c")).toEqual(one);
  });
});

describe("withComposerQuotes", () => {
  it("sends the quotes as > blocks before what was typed", () => {
    expect(
      withComposerQuotes("¿Qué método usa?", [
        { id: "a", text: "First\nquote" },
        { id: "b", text: "Second" },
      ]),
    ).toBe("> First\n> quote\n\n> Second\n\n¿Qué método usa?");
  });

  it("sends only the quotes when nothing was typed", () => {
    expect(withComposerQuotes("  ", [{ id: "a", text: "Only" }])).toBe(
      "> Only",
    );
  });

  it("leaves a message without quotes as it is", () => {
    expect(withComposerQuotes("hola", [])).toBe("hola");
  });
});
