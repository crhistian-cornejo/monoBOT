// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  addBotNews,
  hasUnseenBotNews,
  listBotNews,
  markBotNewsSeen,
  NEWS_ALERT_INTERVAL_MS,
  removeBotNewsFor,
  shouldAlertNews,
} from "./botNews";

describe("bot news", () => {
  beforeEach(() => {
    localStorage.clear();
    markBotNewsSeen();
    for (const item of listBotNews()) removeBotNewsFor(item.monoId);
  });

  it("keeps the newest post first and unseen until read", () => {
    addBotNews({ monoId: "a", botName: "Finance", source: "Bank", text: "1", at: 1 });
    addBotNews({ monoId: "b", botName: "Research", source: "Digest", text: "2", at: 2 });
    expect(listBotNews().map((item) => item.text)).toEqual(["2", "1"]);
    expect(hasUnseenBotNews()).toBe(true);
    markBotNewsSeen();
    expect(hasUnseenBotNews()).toBe(false);
  });

  it("keeps a repeat of a recent post out of the feed", () => {
    expect(
      addBotNews({ monoId: "a", botName: "F", source: "Bank", text: "Same  thing", at: 1 }),
    ).not.toBeNull();
    expect(
      addBotNews({ monoId: "a", botName: "F", source: "Bank", text: "same thing", at: 2 }),
    ).toBeNull();
    // Another bot may say it, and so may this one a day later.
    expect(
      addBotNews({ monoId: "b", botName: "R", source: "Digest", text: "same thing", at: 3 }),
    ).not.toBeNull();
    expect(
      addBotNews({
        monoId: "a",
        botName: "F",
        source: "Bank",
        text: "same thing",
        at: 1 + 25 * 60 * 60 * 1000,
      }),
    ).not.toBeNull();
  });

  it("alerts at once for attention, otherwise at most once an hour", () => {
    const now = 10 * NEWS_ALERT_INTERVAL_MS;
    expect(shouldAlertNews("progress", now)).toBe(true);
    expect(shouldAlertNews("progress", now + 1_000)).toBe(false);
    expect(shouldAlertNews("needs_attention", now + 2_000)).toBe(true);
    expect(shouldAlertNews("blocked", now + 3_000)).toBe(true);
    expect(shouldAlertNews("done", now + NEWS_ALERT_INTERVAL_MS)).toBe(true);
  });

  it("drops a deleted bot's posts", () => {
    addBotNews({ monoId: "a", botName: "Finance", source: "Bank", text: "x" });
    addBotNews({ monoId: "b", botName: "Research", source: "Digest", text: "y" });
    removeBotNewsFor("a");
    expect(listBotNews().map((item) => item.monoId)).toEqual(["b"]);
  });
});
