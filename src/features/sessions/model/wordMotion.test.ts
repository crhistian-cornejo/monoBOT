import { describe, expect, it } from "vitest";
import {
  holdHalfTable,
  PACED_STAGGER_CAP_MS,
  revealEnd,
  WORD_FOCUS_MS,
  WORD_TRAIL_MS,
  wordFadeLingerMs,
  wordReads,
  wordStaggerMs,
} from "./wordMotion";

describe("wordReads", () => {
  it("reads words and numbers, not Markdown marks on their own", () => {
    expect(wordReads("Hello")).toBe(true);
    expect(wordReads("42")).toBe(true);
    expect(wordReads("**bold**")).toBe(true);
    for (const mark of ["-", "*", "##", "|", ">", "1.", "2)", "|---|"]) {
      expect(wordReads(mark)).toBe(false);
    }
  });
});

describe("revealEnd with marks", () => {
  it("lets a list's bullet out with its first word", () => {
    const text = "Intro\n- item one";
    expect(revealEnd(text, 6, true)).toBe(text.indexOf(" one"));
  });

  it("lets a heading's hashes and a number out with the word after", () => {
    expect(revealEnd("## Title here", 0, true)).toBe(8);
    expect(revealEnd("1. First step", 0, true)).toBe(8);
  });

  it("holds a bullet back while its word is still being written", () => {
    expect(revealEnd("Intro\n- ", 6, true)).toBe(6);
    expect(revealEnd("Intro\n- it", 6, true)).toBe(6);
    expect(revealEnd("Intro\n## ", 7, true)).toBe(6);
  });

  it("lets everything out once the stream has ended", () => {
    expect(revealEnd("Intro\n- ", 6, false)).toBe(8);
  });
});

describe("holdHalfTable", () => {
  const header = "Before\n\n| Name | Size |\n";

  it("holds a table back until its header and delimiter have arrived", () => {
    expect(holdHalfTable(header)).toBe("Before\n\n");
    expect(holdHalfTable(`${header}| --- |`)).toBe("Before\n\n");
  });

  it("shows the table once it is one, rows arriving after it", () => {
    const table = `${header}| --- | :---: |\n| a | 1`;
    expect(holdHalfTable(table)).toBe(table);
  });

  it("leaves text that does not end in a table alone", () => {
    const text = "Plain text\nand more";
    expect(holdHalfTable(text)).toBe(text);
    expect(holdHalfTable(`${header}| --- | --- |\n\nAfter`)).toBe(
      `${header}| --- | --- |\n\nAfter`,
    );
  });

  it("never holds lines inside an open code fence", () => {
    const text = "```sh\n| not a table";
    expect(holdHalfTable(text)).toBe(text);
  });

  it("counts escaped bars as text, not as cells", () => {
    expect(holdHalfTable("| a \\| b | c |\n| --- | --- |")).toBe(
      "| a \\| b | c |\n| --- | --- |",
    );
  });
});

describe("timing", () => {
  it("staggers the words of one paint, within a cap", () => {
    expect(wordStaggerMs(0, PACED_STAGGER_CAP_MS)).toBe(0);
    expect(wordStaggerMs(1, PACED_STAGGER_CAP_MS)).toBe(20);
    expect(wordStaggerMs(50, PACED_STAGGER_CAP_MS)).toBe(PACED_STAGGER_CAP_MS);
  });

  it("keeps word spans until the trail has ended", () => {
    expect(wordFadeLingerMs(WORD_FOCUS_MS)).toBe(WORD_TRAIL_MS);
    expect(wordFadeLingerMs(2_000)).toBe(2_000);
  });
});
