// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StreamedPlainText } from "./StreamedPlainText";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  // Unpaced: words show as they arrive, so the test needs no animation clock.
  localStorage.setItem("monocode.pacedReveal", "0");
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  localStorage.removeItem("monocode.pacedReveal");
  vi.useRealTimers();
});

const words = () => [...container.querySelectorAll("[data-word-fade]")];

describe("StreamedPlainText", () => {
  it("fades each word in and keeps a word's element as the text grows", async () => {
    const render = (text: string, streaming: boolean, onSettled?: () => void) =>
      act(() => root.render(createElement(StreamedPlainText, { text, streaming, onSettled })));
    render("Hola Jorge, ", true);
    const first = words()[0];
    expect(words().map((word) => word.textContent)).toEqual(["Hola", "Jorge,"]);
    expect(container.firstElementChild?.classList.contains("word-fading")).toBe(true);

    render("Hola Jorge, gracias\n\nSaludos", true);
    expect(words()[0]).toBe(first);
    expect(container.textContent).toContain("gracias\n\n");

    vi.useFakeTimers();
    const settled = vi.fn();
    render("Hola Jorge, gracias\n\nSaludos", false, settled);
    await act(async () => vi.advanceTimersByTime(5_000));
    expect(settled).toHaveBeenCalled();
    expect(words()).toHaveLength(0);
    expect(container.textContent).toBe("Hola Jorge, gracias\n\nSaludos");
  });
});
