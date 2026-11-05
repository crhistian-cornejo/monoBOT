// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import type { Block } from "../sessions/model/session";
import {
  documentGroups,
  documentStarsSnapshot,
  groupDay,
  toggleDocumentStar,
} from "./documentGroups";

const user = (id: string, text: string, startedAt: number, cards: Block["artifactCards"]): Block => ({
  id,
  role: "user",
  text,
  startedAt,
  artifactCards: cards,
});

describe("document groups", () => {
  it("keeps each request's documents together, final report first, newest request first", () => {
    const groups = documentGroups([
      user("u1", "Pozas de paso\nmore detail", 1, [
        { id: "paper-a", kind: "document", title: "Clark (2009)" },
        { id: "report", kind: "document", title: "Informe", final: true },
      ]),
      { id: "a1", role: "assistant", text: "Listo" },
      user("u2", "Otra búsqueda", 2, [
        { id: "paper-b", kind: "document", title: "Jones (1989)" },
      ]),
    ]);
    expect(groups.map((group) => group.title)).toEqual([
      "Otra búsqueda",
      "Pozas de paso",
    ]);
    expect(groups[1].cards.map((card) => card.id)).toEqual(["report", "paper-a"]);
    expect(groups[1].at).toBe(1);
  });

  it("moves a document revised later to the request that revised it", () => {
    const card = { id: "doc", kind: "document" as const, title: "Draft" };
    const groups = documentGroups([
      user("u1", "First", 1, [card]),
      user("u2", "Revise it", 2, [card]),
    ]);
    expect(groups.map((group) => group.id)).toEqual(["u2"]);
  });

  it("names a habit's documents after the habit", () => {
    const [group] = documentGroups([
      {
        id: "h",
        role: "assistant",
        text: "Report",
        monoHabit: { id: "x", name: "Weekly digest", at: 5 },
        artifactCards: [{ id: "d", kind: "document", title: "Digest" }],
      },
    ]);
    expect(group).toMatchObject({ title: "Weekly digest", at: 5 });
  });

  it("labels the day of a folder", () => {
    const now = new Date(2026, 10, 5, 12).getTime();
    expect(groupDay(new Date(2026, 10, 5, 1).getTime(), now)).toBe("Today");
    expect(groupDay(new Date(2026, 10, 4, 23).getTime(), now)).toBe("Yesterday");
    expect(groupDay(undefined, now)).toBe("Earlier");
  });

  it("stars a document and unstars it", () => {
    toggleDocumentStar("doc");
    expect(documentStarsSnapshot().has("doc")).toBe(true);
    toggleDocumentStar("doc");
    expect(documentStarsSnapshot().has("doc")).toBe(false);
  });
});
