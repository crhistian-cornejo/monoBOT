// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Block } from "../../sessions/model/session";
import { MonoActivityView } from "./MonoActivityView";

let container: HTMLDivElement;
let root: Root;
const onApproval = vi.fn();

beforeEach(() => {
  vi.clearAllMocks();
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function render(blocks: Block[], live = false) {
  act(() =>
    root.render(
      createElement(MonoActivityView, {
        blocks,
        live,
        cwd: "/repo",
        onApproval,
      }),
    ),
  );
}

it("shows the work in order and leaves the answer to the chat", () => {
  render([
    { id: "user", role: "user", text: "Update my instructions" },
    {
      id: "thinking",
      role: "reasoning",
      text: "Check the current instructions.",
    },
    { id: "intro", role: "assistant", text: "I will check what is there." },
    {
      id: "read",
      role: "tool",
      text: "monocode app soul.read",
      tool: {
        kind: "shell",
        status: "completed",
        detail: "Current soul contents",
      },
    },
    { id: "progress", role: "assistant", text: "Now I will update it." },
    {
      id: "write",
      role: "tool",
      text: "monocode app soul.update",
      tool: { kind: "shell", status: "completed", detail: "Saved soul" },
    },
    { id: "reply", role: "assistant", text: "Your instructions are updated." },
  ]);
  expect(
    Array.from(container.querySelectorAll("[data-mono-activity-block]")).map(
      (el) => el.getAttribute("data-mono-activity-block"),
    ),
  ).toEqual(["thinking", "intro", "read", "progress", "write"]);
});

it("sums up the searches and cites the sources the answer used", () => {
  render([
    { id: "user", role: "user", text: "Find a paper", durationMs: 42_000 },
    {
      id: "search",
      role: "tool",
      text: "Find hydrology R",
      tool: {
        kind: "search",
        status: "completed",
        preview: { kind: "search", query: "hydrology R" },
        detail:
          'Links: [{"title":"airGR","url":"https://hal.science/a"},{"title":"Other","url":"https://example.com/b"}]\n',
      },
    },
    {
      id: "reply",
      role: "assistant",
      text: 'It exists [1](https://hal.science/a "airGR | A suite of models.").',
    },
  ]);
  const research = container.querySelector('[aria-label="Research"]');
  expect(research?.textContent).toContain("1 search");
  expect(research?.textContent).toContain("1 cited");
  expect(research?.textContent).toContain("1 passed over");
  expect(research?.textContent).toContain("hydrology R");
  expect(research?.textContent).toContain("airGR");
  expect(container.textContent).toContain("in 42s");
});

it("updates a live trail and keeps approval controls actionable", () => {
  const blocks: Block[] = [
    { id: "user", role: "user", text: "Inspect" },
    {
      id: "call",
      role: "tool",
      text: "ls",
      tool: { kind: "shell", status: "in_progress", detail: "Partial output" },
    },
  ];
  render(blocks, true);
  const call = container.querySelector('[data-mono-activity-block="call"]');
  render(
    [
      blocks[0],
      {
        ...blocks[1],
        tool: { kind: "shell", status: "completed", detail: "Complete output" },
      },
      {
        id: "approval",
        role: "tool",
        text: "monocode app soul.update",
        tool: { kind: "shell", status: "pending" },
        approval: { requestId: 42 },
      },
    ],
    true,
  );
  expect(container.querySelector('[data-mono-activity-block="call"]')).toBe(call);
  expect(container.querySelectorAll("[data-mono-activity-block]")).toHaveLength(2);
  expect(container.textContent).toContain("Working");
  const allow = Array.from(container.querySelectorAll("button")).find(
    (button) => button.textContent === "Allow",
  )!;
  act(() => allow.click());
  expect(onApproval).toHaveBeenCalledWith(42, "allow");
});
