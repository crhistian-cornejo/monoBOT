import { describe, expect, it } from "vitest";
import type { Block, Session } from "../../sessions/model/session";
import {
  ROUND_TIMEOUT_MS,
  roundAnswer,
  roundAnswers,
  roundPrompt,
  roundRequest,
  type GroupRound,
} from "./groupRound";

const ask = (request: string): Block => ({
  id: request,
  role: "user",
  text: "<group_message>…",
  appRequestId: request,
});
const said = (id: string, text: string): Block => ({
  id,
  role: "assistant",
  text,
});

describe("a group's round", () => {
  const docs = roundRequest("g1", "k1", "docs");
  const asis = roundRequest("g1", "k1", "asis");
  const round: GroupRound = {
    key: "k1",
    groupSessionId: "g1",
    text: "¿Están listos?",
    startedAt: 0,
    asks: [
      { request: docs, memberId: "docs", name: "Documents" },
      { request: asis, memberId: "asis", name: "Asistente" },
    ],
    failed: [],
  };

  it("names each member's part after the group chat", () => {
    expect(docs).toBe("app-g1-round-k1-docs");
  });

  it("reads a member's part once its turn is over, PASS as nothing", () => {
    const blocks = [ask(docs), said("a", "Listo"), said("b", "Tengo el PDF")];
    expect(roundAnswer({ busy: true, blocks }, docs)).toBeUndefined();
    expect(roundAnswer({ busy: false, blocks }, docs)).toEqual({
      text: "Tengo el PDF",
    });
    expect(roundAnswer({ busy: false, blocks: [] }, docs)).toBeUndefined();
    expect(
      roundAnswer({ busy: false, blocks: [ask(docs), said("a", "PASS")] }, docs),
    ).toEqual({ text: "" });
  });

  it("waits for every member, unless the round ran out of time", () => {
    const sessions: Record<string, Pick<Session, "blocks" | "busy">> = {
      docs: { busy: false, blocks: [ask(docs), said("a", "Listo")] },
      asis: { busy: true, blocks: [ask(asis)] },
    };
    expect(roundAnswers(round, (id) => sessions[id], 1)).toBeUndefined();
    expect(
      roundAnswers(round, (id) => sessions[id], ROUND_TIMEOUT_MS + 1),
    ).toEqual([
      { name: "Documents", text: "Listo" },
      { name: "Asistente", missing: "still working" },
    ]);
    expect(
      roundAnswers({ ...round, failed: [asis] }, (id) => sessions[id], 1),
    ).toEqual([
      { name: "Documents", text: "Listo" },
      { name: "Asistente", missing: "could not take the message" },
    ]);
  });

  it("hands the lead every answer and the last word", () => {
    const prompt = roundPrompt("¿Están listos?", [
      { name: "Documents", text: "Listo" },
      { name: "Asistente", text: "" },
      { name: "Finance", missing: "still working" },
    ]);
    expect(prompt).toContain('The user wrote to the group: "¿Están listos?"');
    expect(prompt).toContain("### Documents\nListo");
    expect(prompt).toContain("### Asistente\nPASS");
    expect(prompt).toContain("(no answer: still working)");
    expect(prompt).toContain("You have the last word");
  });
});
