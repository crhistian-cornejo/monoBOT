// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import type { Block } from "../sessions/model/session";
import {
  chatTopics,
  documentGroups,
  documentStarsSnapshot,
  filePathsIn,
  groupDay,
  pendingTopicTurns,
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
    expect(groups.map((group) => group.id)).toEqual(["turn:u2"]);
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

  it("groups requests by their topic, and a continue with the request before it", () => {
    const blocks: Block[] = [
      user("u1", "Busca papers de pozas de paso", 1, [
        { id: "paper-a", kind: "document", title: "Clark (2009)" },
      ]),
      user("u2", "Ahora HEC-HMS por código", 2, [
        { id: "hms", kind: "document", title: "HEC-HMS" },
      ]),
      user("u3", "Continue from where you left off.", 3, [
        { id: "hms-2", kind: "document", title: "HEC-HMS scripts" },
      ]),
      user("u4", "Más sobre sedimentación", 4, [
        { id: "report", kind: "document", title: "Informe", final: true },
      ]),
    ];
    const unnamed = documentGroups(blocks);
    expect(unnamed.map((group) => group.title)).toEqual([
      "Más sobre sedimentación",
      "Ahora HEC-HMS por código",
      "Busca papers de pozas de paso",
    ]);
    expect(unnamed[1].cards.map((card) => card.id)).toEqual(["hms", "hms-2"]);

    const topics = {
      u1: "Diseño de pozas de sedimentación",
      u2: "Automatización de HEC-HMS",
      u4: "diseño de pozas de sedimentación",
    };
    const named = documentGroups(blocks, topics);
    expect(named.map((group) => group.title)).toEqual([
      "Diseño de pozas de sedimentación",
      "Automatización de HEC-HMS",
    ]);
    expect(named[0].cards.map((card) => card.id)).toEqual(["report", "paper-a"]);
    expect(named[0].at).toBe(4);
    expect(chatTopics(blocks, topics)).toEqual([
      "Diseño de pozas de sedimentación",
      "Automatización de HEC-HMS",
    ]);
  });

  it("asks a topic only for requests that need one", () => {
    const blocks: Block[] = [
      user("u1", "Pozas de paso", 1, [{ id: "a", kind: "document", title: "A" }]),
      { id: "r1", role: "assistant", text: "Encontré tres guías." },
      user("u2", "Continue from where you left off.", 2, [
        { id: "b", kind: "document", title: "B" },
      ]),
      user("u3", "Hola", 3, []),
    ];
    expect(pendingTopicTurns(blocks, {})).toEqual([
      {
        turnId: "u1",
        request: "Pozas de paso",
        reply: "Encontré tres guías.",
        documents: ["A"],
      },
    ]);
    expect(pendingTopicTurns(blocks, { u1: "Pozas" })).toEqual([]);
  });

  it("lists the files a turn wrote or named beside its documents", () => {
    const blocks: Block[] = [
      user("u1", "Hazlo en Word y Excel", 1, []),
      {
        id: "t1",
        role: "tool",
        text: "",
        tool: {
          kind: "execute",
          status: "completed",
          title: "python make.py --out C:/Users/me/Documents/MonoBOT/Informe.docx",
        },
      },
      {
        id: "a1",
        role: "assistant",
        text: "Guardé la hoja en [aquí](</C:/Users/me/Documents/MonoBOT/Cálculo pozas.xlsx>) y `C:/x/notes.txt`.",
      },
    ];
    const [group] = documentGroups(blocks);
    expect(group.files).toEqual([
      "C:/Users/me/Documents/MonoBOT/Informe.docx",
      "C:/Users/me/Documents/MonoBOT/Cálculo pozas.xlsx",
    ]);
  });

  it("finds Windows and POSIX file paths in text", () => {
    expect(
      filePathsIn(
        "Saved to C:\\Reports\\pozas.pdf, also /home/me/out/data.csv and `D:/A B/c.pptx`.",
      ),
    ).toEqual(["D:/A B/c.pptx", "C:\\Reports\\pozas.pdf", "/home/me/out/data.csv"]);
  });
});
