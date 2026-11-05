import { describe, expect, it } from "vitest";
import {
  citationHost,
  citationNumber,
  collectCitations,
  isCitationHref,
  parseCitationTitle,
} from "./citations";

describe("citations", () => {
  it("reads the number of a citation label and nothing else", () => {
    expect(citationNumber("1")).toBe(1);
    expect(citationNumber(" [12] ")).toBe(12);
    expect(citationNumber("MDN")).toBeNull();
    expect(citationNumber("2024")).toBeNull();
    expect(citationNumber("1.2")).toBeNull();
  });

  it("treats only web links as citations", () => {
    expect(isCitationHref("https://example.com/a")).toBe(true);
    expect(isCitationHref("http://example.com")).toBe(true);
    expect(isCitationHref("src/app/App.tsx")).toBe(false);
    expect(isCitationHref("javascript:alert(1)")).toBe(false);
    expect(isCitationHref(undefined)).toBe(false);
  });

  it("names the site by its host without www", () => {
    expect(citationHost("https://www.web.dev/articles/x")).toBe("web.dev");
    expect(citationHost("not a url")).toBe("not a url");
  });

  it("splits the title from the supporting passage", () => {
    expect(
      parseCitationTitle(
        "Great animations | UI motion should be fast.",
        "https://emilkowal.ski/a",
      ),
    ).toEqual({
      title: "Great animations",
      snippet: "UI motion should be fast.",
    });
  });

  it("collects distinct sources numbered by first citation", () => {
    const sources = collectCitations(
      'A [4](https://a.dev/x "Alpha") B [[2]](https://b.dev/y "Beta | Passage") C [9](https://a.dev/x "Alpha | Late passage").',
    );
    expect(sources).toEqual([
      {
        url: "https://a.dev/x",
        host: "a.dev",
        number: 1,
        title: "Alpha",
        snippet: "Late passage",
      },
      {
        url: "https://b.dev/y",
        host: "b.dev",
        number: 2,
        title: "Beta",
        snippet: "Passage",
      },
    ]);
  });

  it("ignores the format when it sits in code or in ordinary links", () => {
    expect(
      collectCitations(
        "Write `[1](https://a.dev)` like this:\n```\n[2](https://b.dev)\n```\nSee [docs](https://c.dev) and [2024](https://d.dev).",
      ),
    ).toEqual([]);
  });

  it("keeps a bare title and falls back to the host without one", () => {
    expect(parseCitationTitle("Only a title", "https://a.dev/x")).toEqual({
      title: "Only a title",
    });
    expect(parseCitationTitle(undefined, "https://www.a.dev/x")).toEqual({
      title: "a.dev",
    });
    expect(parseCitationTitle(" | just a passage", "https://a.dev/x")).toEqual({
      title: "a.dev",
      snippet: "just a passage",
    });
  });
});
