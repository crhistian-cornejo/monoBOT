import { describe, expect, it } from "vitest";
import {
  agentMessage,
  ANSWERING_WINDOW_MS,
  isAnswering,
  markAnswering,
} from "./agentMessages";

describe("agent messages", () => {
  it("signs the message and keeps the recipient from messaging on", () => {
    const text = agentMessage("Finance", "Finance bot", " How much on delivery? ");
    expect(text).toContain('<agent_message from="Finance" role="Finance bot">');
    expect(text).toContain("How much on delivery?");
    expect(text).toContain("do not message other agents while answering it");
  });

  it("counts a session as answering for a while after it is written to", () => {
    markAnswering("s1", 1_000);
    expect(isAnswering("s1", 2_000)).toBe(true);
    expect(isAnswering("s1", 1_000 + ANSWERING_WINDOW_MS + 1)).toBe(false);
    expect(isAnswering("other", 2_000)).toBe(false);
  });
});
