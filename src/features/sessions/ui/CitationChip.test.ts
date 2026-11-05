// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const openUrl = vi.hoisted(() => vi.fn(() => Promise.resolve()));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl }));

import { AgentMarkdown } from "./AgentMarkdown";

describe("inline citations", () => {
  let root: Root;
  let container: HTMLDivElement;

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    openUrl.mockClear();
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  async function render(
    text: string,
    citations?: "chips" | "sources",
  ) {
    await act(async () =>
      root.render(
        createElement(AgentMarkdown, { text, cwd: "/repo", citations }),
      ),
    );
  }

  const CITED =
    'Keep it quick [1](https://www.web.dev/articles/a "Animations and performance | Stick to transform and opacity.").';
  const chip = () =>
    container.querySelector<HTMLAnchorElement>('a[data-citation]');
  const card = () => document.body.querySelector('[role="tooltip"]');

  it("turns a numbered web link into a source chip named by its host", async () => {
    await render(CITED, "chips");
    expect(chip()?.getAttribute("aria-label")).toBe(
      "Source 1: Animations and performance",
    );
    expect(chip()?.textContent).toBe("wweb.dev");
  });

  it("leaves numbered links as links unless the text opted in", async () => {
    await render(CITED);
    expect(chip()).toBeNull();
    expect(container.querySelector("a")?.textContent).toBe("1");
  });

  it("previews the title and the passage on focus", async () => {
    await render(CITED, "chips");
    expect(card()).toBeNull();
    await act(async () => chip()?.focus());
    expect(card()?.textContent).toContain("Animations and performance");
    expect(card()?.textContent).toContain("Stick to transform and opacity.");
    await act(async () => chip()?.blur());
    expect(card()).toBeNull();
  });

  it("opens the source in the browser on click", async () => {
    await render(CITED, "chips");
    await act(async () => chip()?.click());
    expect(openUrl).toHaveBeenCalledWith("https://www.web.dev/articles/a");
  });

  it("leaves ordinary links as links", async () => {
    await render(
      "See [the docs](https://example.com/docs) and [2024](https://example.com/y).",
      "sources",
    );
    expect(
      [...container.querySelectorAll("a")].map((link) => link.textContent),
    ).toEqual(["the docs", "2024"]);
    expect(chip()).toBeNull();
    expect(container.textContent).not.toContain("source");
  });

  describe("sources list", () => {
    const TWO =
      'One [7](https://a.dev/x "Alpha | First passage.") and two [2](https://b.dev/y "Beta") and again [9](https://a.dev/x "Alpha").';

    it("numbers the sources by first citation and counts them once", async () => {
      await render(TWO, "sources");
      const chips = [...container.querySelectorAll("a[data-citation]")];
      expect(chips.map((el) => el.getAttribute("aria-label"))).toEqual([
        "Source 1: Alpha",
        "Source 2: Beta",
        "Source 1: Alpha",
      ]);
      expect(container.textContent).toContain("2 sources");
    });

    it("is collapsed until opened and lists title, host and number", async () => {
      await render(TWO, "sources");
      const toggle = container.querySelector<HTMLButtonElement>(
        "button[aria-expanded]",
      );
      const list = container.querySelector("ol")?.parentElement;
      expect(toggle?.getAttribute("aria-expanded")).toBe("false");
      expect(list?.hasAttribute("inert")).toBe(true);
      await act(async () => toggle?.click());
      expect(toggle?.getAttribute("aria-expanded")).toBe("true");
      expect(list?.hasAttribute("inert")).toBe(false);
      const rows = [...container.querySelectorAll("ol li")].map(
        (li) => li.textContent,
      );
      expect(rows[0]).toContain("Alpha");
      expect(rows[0]).toContain("a.dev");
      expect(rows[1]).toContain("Beta");
    });

    it("stays out of a text that only asked for chips", async () => {
      await render(TWO, "chips");
      expect(container.querySelector("ol")).toBeNull();
    });
  });
});
