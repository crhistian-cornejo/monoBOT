import { describe, expect, it } from "vitest";
import { addressedMember, mentionsIn, rankMentionPeople } from "./groupMentions";

const members = [
  { id: "fin", name: "Finance" },
  { id: "docs", name: "Documents" },
  { id: "asis", name: "Mi Asistente" },
  { id: "mi", name: "Mi" },
];
const lead = { id: "ivy", name: "Ivy" };

describe("addressedMember", () => {
  it("sends a message that opens with @Name to that member, without the name", () => {
    expect(addressedMember("@Finance ¿cuánto gasté este mes?", members, lead)).toEqual({
      kind: "member",
      member: members[0],
      message: "¿cuánto gasté este mes?",
    });
    expect(addressedMember("@finance, revisa el correo", members, lead)).toMatchObject({
      message: "revisa el correo",
    });
  });

  it("finds the name anywhere and keeps it as a plain word", () => {
    expect(addressedMember("oye @Finance, ¿cuánto gasté?", members, lead)).toEqual({
      kind: "member",
      member: members[0],
      message: "oye Finance, ¿cuánto gasté?",
    });
  });

  it("prefers the longest name, spaces included", () => {
    expect(addressedMember("@Mi Asistente agenda la reunión", members)).toMatchObject({
      member: members[2],
      message: "agenda la reunión",
    });
    expect(addressedMember("@Mi dime algo", members)).toMatchObject({ member: members[3] });
  });

  it("gives the lead its own messages, without a round", () => {
    expect(addressedMember("@Ivy ¿qué opinas?", members, lead)).toEqual({ kind: "lead" });
  });

  it("leaves several people, nobody, or only a name to the whole group", () => {
    expect(addressedMember("@Finance y @Documents hagan el Excel", members, lead)).toBeUndefined();
    expect(addressedMember("@Finance y @Ivy", members, lead)).toBeUndefined();
    expect(addressedMember("¿qué hacemos hoy?", members, lead)).toBeUndefined();
    expect(addressedMember("@Finance", members, lead)).toBeUndefined();
    expect(addressedMember("@Financiero dime", members, lead)).toBeUndefined();
  });

  it("counts one member named twice as one", () => {
    expect(addressedMember("@Finance mira esto, @Finance", members)).toMatchObject({
      member: members[0],
      message: "mira esto, Finance",
    });
  });

  it("does not take an email address for a mention", () => {
    expect(mentionsIn("escribe a ana@Finance.com", members)).toEqual([]);
  });

  it("ignores accents and case", () => {
    expect(addressedMember("@ASISTENTE hola", [{ id: "a", name: "Asistente" }])).toMatchObject({
      message: "hola",
    });
    expect(addressedMember("@Análisis hola", [{ id: "a", name: "Analisis" }])).toMatchObject({
      message: "hola",
    });
  });
});

describe("rankMentionPeople", () => {
  it("lists names that start with what was typed first", () => {
    expect(rankMentionPeople(members, "as").map((p) => p.id)).toEqual(["asis"]);
    expect(rankMentionPeople(members, "mi").map((p) => p.id)).toEqual(["asis", "mi"]);
    expect(rankMentionPeople(members, "nan").map((p) => p.id)).toEqual(["fin"]);
    expect(rankMentionPeople(members, "").length).toBe(4);
  });
});
