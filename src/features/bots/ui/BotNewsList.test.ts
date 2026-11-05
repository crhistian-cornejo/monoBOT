// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  addBotNews,
  listBotNews,
  removeBotNewsFor,
} from "../model/botNews";
import { BotNewsList } from "./BotNewsList";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

let host: HTMLDivElement;
beforeEach(() => {
  for (const item of listBotNews()) removeBotNewsFor(item.monoId);
  localStorage.setItem(
    "monocode:mono-roster",
    JSON.stringify([
      { id: "social", name: "Social Media", mascot: "ghost", color: "blue", projects: [], kind: "bot", botRole: "social" },
      { id: "fin", name: "Finance", mascot: "crab", color: "red", projects: [], kind: "bot", botRole: "finance" },
    ]),
  );
  addBotNews({ monoId: "fin", botName: "Finance", source: "Daily", text: "Two Netflix charges", at: 1, status: "needs_attention" });
  addBotNews({ monoId: "social", botName: "Social Media", source: "Radar", text: "GPT-6 Cyber", at: 2, sessionId: "social-chat", blockId: "post-1" });
  host = document.createElement("div");
  document.body.append(host);
});
afterEach(() => {
  host.remove();
  localStorage.clear();
});

const buttons = (label: string) =>
  [...host.querySelectorAll("button")].filter((button) =>
    button.textContent?.includes(label),
  );

it("signs each report with its bot and opens it where it was posted", () => {
  const onOpenBot = vi.fn();
  const root = createRoot(host);
  act(() => root.render(createElement(BotNewsList, { onOpenBot })));
  expect(host.querySelectorAll("[data-bot-news] li")).toHaveLength(2);
  expect(host.querySelectorAll("[data-bot-news] svg.pixel-mascot")).toHaveLength(2);
  // A report that needs the user asks for a reply; it had no place saved.
  expect(buttons("Reply in chat")).toHaveLength(1);
  act(() => buttons("Open in chat")[0]!.click());
  expect(onOpenBot).toHaveBeenLastCalledWith("social", {
    sessionId: "social-chat",
    blockId: "post-1",
  });
  act(() => buttons("Reply in chat")[0]!.click());
  expect(onOpenBot).toHaveBeenLastCalledWith("fin", undefined);
  act(() => root.unmount());
});

const chip = (label: string) =>
  host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;

it("filters the feed to one bot, its chip only its face and count", () => {
  const root = createRoot(host);
  act(() => root.render(createElement(BotNewsList, { onOpenBot: () => {} })));
  const finance = chip("Finance · 1");
  expect(finance.title).toBe("Finance");
  expect(finance.textContent).toBe("1");
  act(() => finance.click());
  const items = host.querySelectorAll("[data-bot-news] li");
  expect(items).toHaveLength(1);
  expect(items[0]!.textContent).toContain("Two Netflix charges");
  act(() => chip("All · 2").click());
  expect(host.querySelectorAll("[data-bot-news] li")).toHaveLength(2);
  act(() => root.unmount());
});

it("shows a short summary of a report when its header is hovered", () => {
  vi.useFakeTimers();
  const root = createRoot(host);
  act(() => root.render(createElement(BotNewsList, { onOpenBot: () => {} })));
  const header = host.querySelector<HTMLButtonElement>(
    'button[aria-label="Open Finance"]',
  )!;
  act(() => {
    header.dispatchEvent(
      new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }),
    );
    vi.advanceTimersByTime(300);
  });
  const card = document.querySelector("[data-news-summary]");
  expect(card?.textContent).toContain("Two Netflix charges");
  // Not the bot's own card any more.
  expect(document.querySelector("[data-mono-preview]")).toBeNull();
  act(() => root.unmount());
  vi.useRealTimers();
});
