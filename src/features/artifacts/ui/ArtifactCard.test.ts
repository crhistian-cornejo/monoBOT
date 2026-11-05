import { describe, expect, it } from "vitest";
import { cardFormat, isPaperCard } from "./ArtifactCard";

describe("the file a card in the chat stands for", () => {
  it("is a PDF for a paper, which the paper tool files under its own prefix", () => {
    expect(isPaperCard("paper-10-1016-j-envsoft-2017-05-002")).toBe(true);
    expect(
      cardFormat({ id: "paper-pdf-0123456789abcdef", kind: "document" }),
    ).toBe("pdf");
  });

  it("is Markdown for a written document and HTML for a visual", () => {
    expect(isPaperCard("artifact-lead-report-1")).toBe(false);
    expect(cardFormat({ id: "artifact-lead-report-1", kind: "document" })).toBe(
      "md",
    );
    expect(cardFormat({ id: "artifact-lead-chart", kind: "html" })).toBe(
      "html",
    );
  });
});
