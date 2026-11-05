// @vitest-environment happy-dom
import { act, createElement, Fragment } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it } from "vitest";
import type { Mono } from "../model/mono";
import { useMonoCardPreview } from "./MonoCardPreview";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const bot = (id: string, name: string): Mono => ({
  id,
  name,
  mascot: "crab",
  color: "red",
  projects: [],
  kind: "bot",
  botRole: "assistant",
});

function Rail() {
  const preview = useMonoCardPreview(new Map());
  return createElement(
    Fragment,
    null,
    ...["asis", "fin"].map((id) =>
      createElement("div", { key: id, id, ...preview.bind(id) }, id),
    ),
    preview.card,
  );
}

let host: HTMLDivElement;
beforeEach(() => {
  localStorage.setItem(
    "monocode:mono-roster",
    JSON.stringify([bot("asis", "Asistente"), bot("fin", "Finance")]),
  );
  host = document.createElement("div");
  document.body.append(host);
});
afterEach(() => {
  host.remove();
  localStorage.clear();
});

const move = (from: Element | null, to: Element) => {
  const init = { bubbles: true, pointerType: "mouse" } as PointerEventInit;
  if (from)
    from.dispatchEvent(new PointerEvent("pointerout", { ...init, relatedTarget: to }));
  to.dispatchEvent(
    new PointerEvent("pointerover", { ...init, relatedTarget: from }),
  );
};

it("keeps the next row's card open when the pointer moves down the rail", () => {
  const root = createRoot(host);
  act(() => root.render(createElement(Rail)));
  const asis = host.querySelector("#asis")!;
  const fin = host.querySelector("#fin")!;
  act(() => move(null, asis));
  expect(document.body.textContent).toContain("Asistente");
  act(() => move(asis, fin));
  expect(document.querySelector("[data-mono-preview]")?.getAttribute(
    "data-mono-preview",
  )).toBe("fin");
  act(() => root.unmount());
});
