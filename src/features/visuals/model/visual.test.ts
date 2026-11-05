import { describe, expect, it } from "vitest";

import type { Block, Session } from "../../sessions/model/session";
import {
  parseVisualDocument,
  placedVisualIds,
  recordVisual,
  sanitizeVisuals,
  splitVisualEmbeds,
  visualAnchor,
  visualsById,
} from "./visual";

const doc = (id: string, title = id) => ({
  id,
  title,
  source: "return () => visual.text('hi');",
});

describe("splitVisualEmbeds", () => {
  it("splits a reply at lines that only embed a visual", () => {
    expect(
      splitVisualEmbeds(
        "Deploys rose.\n![Weekly deploys](visual:deploys)\nThen fell.",
      ),
    ).toEqual([
      { kind: "text", text: "Deploys rose." },
      { kind: "visual", id: "deploys", caption: "Weekly deploys" },
      { kind: "text", text: "Then fell." },
    ]);
  });

  it("leaves embeds inside code and inside sentences as text", () => {
    const text =
      "```md\n![a](visual:a)\n```\nSee ![b](visual:b) here.\n~~~\n![c](visual:c)\n~~~";
    expect(splitVisualEmbeds(text)).toEqual([{ kind: "text", text }]);
  });

  it("keeps a reply without embeds whole", () => {
    expect(splitVisualEmbeds("plain\n\ntext")).toEqual([
      { kind: "text", text: "plain\n\ntext" },
    ]);
  });
});

describe("documents", () => {
  it("rejects malformed visuals and trims titles", () => {
    expect(parseVisualDocument({ ...doc("a"), title: "  A  " })?.title).toBe(
      "A",
    );
    expect(parseVisualDocument({ ...doc("a b") })).toBeUndefined();
    expect(parseVisualDocument({ ...doc("a"), source: " " })).toBeUndefined();
    expect(
      parseVisualDocument({ ...doc("a"), source: "x".repeat(70_000) }),
    ).toBeUndefined();
    expect(sanitizeVisuals([doc("a"), null, doc("a", "again")])).toEqual([
      doc("a", "again"),
    ]);
    expect(sanitizeVisuals("nope")).toEqual([]);
  });

  it("records on the latest user message and moves a republished id", () => {
    const blocks: Block[] = [
      { id: "u1", role: "user", text: "first" },
      { id: "a1", role: "assistant", text: "ok" },
      { id: "u2", role: "user", text: "second" },
    ];
    let session = { id: "s", blocks } as unknown as Session;
    expect(visualAnchor(session)).toBe("u2");
    session = recordVisual(session, "u1", doc("chart"));
    session = recordVisual(session, "u2", doc("chart", "Revised"));
    expect(session.blocks[0].visuals).toBeUndefined();
    expect(session.blocks[2].visuals).toEqual([doc("chart", "Revised")]);
    expect(visualsById(session.blocks).get("chart")?.title).toBe("Revised");
  });

  it("knows which visuals a turn's replies placed", () => {
    const turn: Block[] = [
      { id: "u", role: "user", text: "![x](visual:fake)" },
      { id: "a", role: "assistant", text: "Look:\n![X](visual:x)" },
    ];
    expect([...placedVisualIds(turn)]).toEqual(["x"]);
  });
});
