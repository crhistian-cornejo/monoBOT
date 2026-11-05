// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { VisualNode } from "../model/visual";
import { VisualView } from "./VisualView";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const calls = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: calls.invoke }));
vi.mock("../../sessions/ui/AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("pre", { "data-source": true }, text),
}));

const tree = (count: number): VisualNode => ({
  kind: "column",
  gap: 12,
  children: [
    { kind: "stat", label: "Count", value: String(count), color: "ink", trend: [1, 3, 2, 5] },
    {
      kind: "chart",
      chart_type: "area",
      height: 200,
      labels: ["Jan", "Feb", "Mar"],
      series: [
        { label: "Revenue", values: [3, 5, count], color: "series1" },
        { label: "Cost", values: [1, 2, 2], color: "series2" },
      ],
    },
    { kind: "button", label: "Add", handler: "1:0", disabled: false },
  ],
});

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  // happy-dom lays nothing out; give boxes a width so charts draw.
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get: () => 480,
  });
  calls.invoke.mockReset();
  calls.invoke.mockImplementation(async (command: string, args: { value?: unknown }) => {
    if (command === "visual_open") return { handle: 7, tree: tree(0) };
    if (command === "visual_dispatch") return tree(1);
    return undefined;
  });
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => {
  await new Promise((resolve) => setTimeout(resolve, 0));
});

describe("VisualView", () => {
  it("runs the program, draws dithered charts and sends control changes", async () => {
    act(() =>
      root.render(
        createElement(VisualView, {
          document: { id: "rev", title: "Revenue", source: "return () => null" },
        }),
      ),
    );
    await flush();
    expect(calls.invoke).toHaveBeenCalledWith("visual_open", {
      source: "return () => null",
    });
    expect(host.querySelector("figcaption")?.textContent).toBe("Revenue");
    // The area chart's cells, its legend and its accessible table.
    expect(host.querySelectorAll("path[shape-rendering=crispEdges]").length).toBeGreaterThan(0);
    expect(host.querySelector("table.sr-only caption")?.textContent).toContain("Revenue");
    expect([...host.querySelectorAll("button[aria-pressed]")].map((b) => b.textContent)).toContain("Cost");

    const add = [...host.querySelectorAll("button")].find((b) => b.textContent === "Add")!;
    act(() => add.click());
    await flush();
    expect(calls.invoke).toHaveBeenCalledWith("visual_dispatch", {
      handle: 7,
      handler: "1:0",
      value: null,
    });
    expect(host.textContent).toContain("Count1");

    act(() => (host.querySelector("[aria-label='Show source']") as HTMLButtonElement).click());
    expect(host.querySelector("[data-source]")?.textContent).toContain("return () => null");
  });

  it("closes the program when it leaves the chat", async () => {
    act(() =>
      root.render(
        createElement(VisualView, {
          document: { id: "a", title: "A", source: "x" },
        }),
      ),
    );
    await flush();
    act(() => root.render(null));
    expect(calls.invoke).toHaveBeenCalledWith("visual_close", { handle: 7 });
  });

  it("says why a program could not run", async () => {
    calls.invoke.mockImplementation(async () => {
      throw "Visual execution failed: boom";
    });
    act(() =>
      root.render(
        createElement(VisualView, {
          document: { id: "a", title: "A", source: "x" },
        }),
      ),
    );
    await flush();
    expect(host.textContent).toContain("boom");
  });
});
