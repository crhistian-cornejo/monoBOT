// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";

const known = vi.hoisted(() => vi.fn());
vi.mock("./paperStore", () => ({ paperKnown: known }));

import {
  announcePaperShown,
  FOCUS_PAPER_EVENT,
  focusPaperPassage,
  isPaperShown,
  OPEN_PAPER_EVENT,
  openCitedPaper,
  openSource,
  paperKeyOf,
  type FocusPaperRequest,
  type OpenPaperRequest,
} from "./openPaper";

afterEach(() => known.mockReset());

function listen() {
  const seen: OpenPaperRequest[] = [];
  const handler = (event: Event) =>
    seen.push((event as CustomEvent<OpenPaperRequest>).detail);
  window.addEventListener(OPEN_PAPER_EVENT, handler);
  return {
    seen,
    stop: () => window.removeEventListener(OPEN_PAPER_EVENT, handler),
  };
}

describe("paperKeyOf", () => {
  it("names the cached paper a citation link points to", () => {
    expect(paperKeyOf("https://doi.org/10.1016/J.ENVSOFT.2017.05.002")).toBe(
      "10.1016/j.envsoft.2017.05.002",
    );
    expect(paperKeyOf("https://arxiv.org/abs/1706.03762v5")).toBe(
      "10.48550/arxiv.1706.03762",
    );
    expect(paperKeyOf("https://example.com/blog/post")).toBeUndefined();
  });
});

describe("openSource", () => {
  const passage =
    "The package allows users to calibrate and run hourly models.";
  const href = "https://doi.org/10.1016/j.envsoft.2017.05.002";

  it("shows a paper the app has read, at the quoted passage, and leaves the browser alone", async () => {
    known.mockResolvedValue(true);
    const fallback = vi.fn(async () => {});
    const { seen, stop } = listen();
    await openSource(href, passage, fallback);
    stop();
    expect(seen).toEqual([{ key: "10.1016/j.envsoft.2017.05.002", passage }]);
    expect(fallback).not.toHaveBeenCalled();
  });

  it("opens in the browser a paper the app never read", async () => {
    known.mockResolvedValue(false);
    const fallback = vi.fn(async () => {});
    const { seen, stop } = listen();
    await openSource(href, passage, fallback);
    stop();
    expect(seen).toEqual([]);
    expect(fallback).toHaveBeenCalledWith(href);
  });

  it("opens an ordinary web source in the browser without asking the cache", async () => {
    const fallback = vi.fn(async () => {});
    await openSource("https://example.com/a", "x", fallback);
    expect(known).not.toHaveBeenCalled();
    expect(fallback).toHaveBeenCalledWith("https://example.com/a");
  });

  it("falls back to the browser when the cache cannot be read", async () => {
    known.mockRejectedValue(new Error("ipc"));
    const fallback = vi.fn(async () => {});
    await openSource(href, passage, fallback);
    expect(fallback).toHaveBeenCalledWith(href);
    expect(await openCitedPaper("https://example.com", undefined)).toBe(false);
  });
});

describe("a paper open beside the chat", () => {
  const href = "https://doi.org/10.1016/j.envsoft.2017.05.002";
  const key = "10.1016/j.envsoft.2017.05.002";
  const passage =
    "The package allows users to calibrate and run hourly models.";

  function listenFocus() {
    const seen: FocusPaperRequest[] = [];
    const handler = (event: Event) =>
      seen.push((event as CustomEvent<FocusPaperRequest>).detail);
    window.addEventListener(FOCUS_PAPER_EVENT, handler);
    return {
      seen,
      stop: () => window.removeEventListener(FOCUS_PAPER_EVENT, handler),
    };
  }

  it("counts the viewers that show it, and forgets them as they close", () => {
    expect(isPaperShown(key)).toBe(false);
    const first = announcePaperShown(key);
    const second = announcePaperShown(key);
    first();
    expect(isPaperShown(key)).toBe(true);
    second();
    expect(isPaperShown(key)).toBe(false);
  });

  it("points that viewer at the cited passage, and says it did", () => {
    const withdraw = announcePaperShown(key);
    const { seen, stop } = listenFocus();
    expect(focusPaperPassage(href, passage)).toBe(true);
    stop();
    withdraw();
    expect(seen).toEqual([{ key, passage }]);
  });

  it("does nothing, and says so, when no viewer shows the paper or the link is not a paper", () => {
    const { seen, stop } = listenFocus();
    expect(focusPaperPassage(href, passage)).toBe(false);
    const withdraw = announcePaperShown(key);
    expect(focusPaperPassage("https://example.com/post", passage)).toBe(false);
    withdraw();
    stop();
    expect(seen).toEqual([]);
  });

  it("clicking a citation focuses the open paper instead of opening another window", async () => {
    const withdraw = announcePaperShown(key);
    const focused = listenFocus();
    const opened = listen();
    const fallback = vi.fn(async () => {});
    await openSource(href, passage, fallback);
    withdraw();
    focused.stop();
    opened.stop();
    expect(focused.seen).toEqual([{ key, passage }]);
    expect(opened.seen).toEqual([]);
    expect(fallback).not.toHaveBeenCalled();
    expect(known).not.toHaveBeenCalled();
  });
});
