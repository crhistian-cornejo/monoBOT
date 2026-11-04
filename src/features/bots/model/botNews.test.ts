// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import {
  addBotNews,
  hasUnseenBotNews,
  listBotNews,
  markBotNewsSeen,
  removeBotNewsFor,
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

  it("drops a deleted bot's posts", () => {
    addBotNews({ monoId: "a", botName: "Finance", source: "Bank", text: "x" });
    addBotNews({ monoId: "b", botName: "Research", source: "Digest", text: "y" });
    removeBotNewsFor("a");
    expect(listBotNews().map((item) => item.monoId)).toEqual(["b"]);
  });
});
