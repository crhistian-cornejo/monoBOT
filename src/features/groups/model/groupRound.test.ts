import { describe, expect, it } from "vitest";
import type { Block, Session } from "../../sessions/model/session";
import {
  ROUND_TIMEOUT_MS,
  isQuickAsk,
  quickRoundOutcome,
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

describe("an errand to the group", () => {
  it.each([
    "Ábreme el informe de la poza P-01",
    "oye, pásame la ruta del plano 0060CIV0003",
    "¿Dónde está la memoria A.2?",
    "Por favor dame el link del PPT de avance",
    "Pásame todos los planos del canal",
    "Busca el RFI 003 y compártemelo",
    "Can you open the hydrology report?",
    "where is the latest deck",
  ])("is an errand: %s", (text) => {
    expect(isQuickAsk(text)).toBe(true);
  });

  it.each([
    "¿Qué opinan de la propuesta de HDPE para la conducción?",
    "Revisen entre todos las observaciones 55 y 57",
    "Tengo una observación sobre el caudal de diseño",
    "Pásame un plan para cerrar el RFI esta semana",
    "Analicen el informe y díganme qué falta",
    "Everyone: compare the two options",
    "Hola, ¿cómo vamos con el proyecto?",
    `Abre el informe y ${"explica cada capítulo con detalle ".repeat(8)}`,
  ])("is for the members together: %s", (text) => {
    expect(isQuickAsk(text)).toBe(false);
  });

  const docs = roundRequest("g1", "k2", "docs", true);
  const asis = roundRequest("g1", "k2", "asis", true);
  const fin = roundRequest("g1", "k2", "fin", true);
  const errand: GroupRound = {
    key: "k2",
    groupSessionId: "g1",
    text: "Pásame la ruta del informe",
    startedAt: 0,
    asks: [
      { request: docs, memberId: "docs", name: "Documents" },
      { request: asis, memberId: "asis", name: "Asistente" },
      { request: fin, memberId: "fin", name: "Finance" },
    ],
    failed: [],
    quick: true,
  };

  it("names its parts apart from a round's", () => {
    expect(docs).toBe("app-g1-quick-k2-docs");
  });

  it("ends with the first member that does it, stopping the ones still at it", () => {
    const sessions: Record<string, Pick<Session, "blocks" | "busy">> = {
      docs: { busy: true, blocks: [ask(docs)] },
      asis: { busy: false, blocks: [ask(asis), said("a", "`D:/x/Informe.pdf`")] },
      fin: { busy: false, blocks: [ask(fin), said("b", "PASS")] },
    };
    expect(quickRoundOutcome(errand, (id) => sessions[id], 1)).toEqual({
      winner: asis,
      stop: [docs],
    });
  });

  it("waits while nobody did it yet, then lets the lead answer", () => {
    const sessions: Record<string, Pick<Session, "blocks" | "busy">> = {
      docs: { busy: true, blocks: [ask(docs)] },
      asis: { busy: false, blocks: [ask(asis), said("a", "PASS")] },
      fin: { busy: false, blocks: [ask(fin), said("b", "PASS")] },
    };
    expect(quickRoundOutcome(errand, (id) => sessions[id], 1)).toBeUndefined();
    sessions.docs = { busy: false, blocks: [ask(docs), said("c", "PASS")] };
    expect(quickRoundOutcome(errand, (id) => sessions[id], 1)).toEqual({
      answers: [
        { name: "Documents", text: "" },
        { name: "Asistente", text: "" },
        { name: "Finance", text: "" },
      ],
    });
  });
});
