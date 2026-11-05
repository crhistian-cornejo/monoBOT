import { describe, expect, it } from "vitest";
import { newSession } from "../sessions/model/session";
import {
  artifactCards,
  recordArtifactCard,
  removeArtifactCard,
} from "./artifacts";

describe("document references", () => {
  it("deduplicates retries and revisions within the original turn", () => {
    const session = newSession("codex", "/tmp");
    session.blocks = [
      { id: "turn", role: "user", text: "Review" },
      { id: "other", role: "user", text: "Later" },
    ];
    const card = {
      id: "artifact-report",
      kind: "document" as const,
      title: "PR review",
    };
    const saved = recordArtifactCard(session, "turn", card);
    const retry = recordArtifactCard(saved, "turn", {
      ...card,
      summary: "Revised",
    });
    expect(artifactCards(retry.blocks)).toEqual([
      { ...card, summary: "Revised" },
    ]);
    expect(retry.blocks[1].artifactCards).toBeUndefined();
    expect(session.blocks[0].artifactCards).toBeUndefined();
    const other = recordArtifactCard(retry, "other", { ...card, id: "keep" });
    const removed = removeArtifactCard(other, card.id);
    expect(artifactCards(removed.blocks)).toEqual([{ ...card, id: "keep" }]);
    expect(removed.blocks[0].artifactCards).toBeUndefined();
    expect(removed.blocks[0].text).toBe("Review");
    expect(removed.blocks[1]).toBe(other.blocks[1]);
    expect(retry.blocks[0].artifactCards).toHaveLength(1);
    expect(removeArtifactCard(removed, card.id)).toBe(removed);
  });
});

describe("paperKeyOfDocument", () => {
  it("reads the paper a dossier stands for, whatever its key holds", async () => {
    const { paperKeyOfDocument } = await import("./ui/ArtifactContent");
    const key = "10.1016/j.envsoft.2017.05.002";
    expect(
      paperKeyOfDocument(
        `# Title\n\nAbstract.\n\n[monobot-paper]: paper:${encodeURIComponent(key)}`,
      ),
    ).toBe(key);
    expect(paperKeyOfDocument("# A report\n\nNo marker here.")).toBeUndefined();
    expect(paperKeyOfDocument("[monobot-paper]: paper:%E0%A4%A")).toBeUndefined();
    // The import brings the whole reader (Markdown, KaTeX, the paper viewer);
    // on a cold, busy run that alone can pass the default 5 seconds.
  }, 30_000);
});
