import { describe, expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  addressedMembers,
  callMessage,
  callTranscript,
  callTurnOrder,
  couldBePass,
  defaultCallLanguage,
  isPass,
  memberReply,
  memberVoices,
  replyLanguage,
  speakable,
  takeSentences,
  type CallLine,
} from "./groupCall";

const members = [
  { id: "m-ana", name: "Ana" },
  { id: "m-leo", name: "Leo" },
  { id: "m-sofia", name: "Sofía" },
  { id: "m-trip", name: "Trip Planner" },
];

describe("who answers on a call", () => {
  it("lets the members the user names answer, in the order named", () => {
    expect(addressedMembers(members, "Sofia y Ana, ¿qué opinan?")).toEqual([
      "m-sofia",
      "m-ana",
    ]);
    expect(addressedMembers(members, "@leo what do you think")).toEqual([
      "m-leo",
    ]);
    expect(addressedMembers(members, "ask the trip planner")).toEqual([
      "m-trip",
    ]);
    // Names are whole words: "Leonardo" is not Leo.
    expect(addressedMembers(members, "Leonardo da Vinci painted")).toEqual([]);
  });

  it("goes round everyone, starting after last round's first speaker", () => {
    expect(callTurnOrder(members, "¿Qué hacemos mañana?")).toEqual({
      order: ["m-ana", "m-leo", "m-sofia", "m-trip"],
      addressed: false,
    });
    expect(callTurnOrder(members, "And then?", "m-leo").order).toEqual([
      "m-sofia",
      "m-trip",
      "m-ana",
      "m-leo",
    ]);
    expect(callTurnOrder(members, "Leo, you first", "m-leo")).toEqual({
      order: ["m-leo"],
      addressed: true,
    });
    expect(callTurnOrder(members, "hi", "gone").order[0]).toBe("m-ana");
    expect(callTurnOrder([], "hi").order).toEqual([]);
  });
});

describe("what a member is told", () => {
  it("frames the call, what was said and who already answered", () => {
    const message = callMessage({
      group: "Trip",
      goal: "Plan the trip to Cusco",
      others: ["Ana", "Leo"],
      said: " ¿Volamos el viernes? ",
      earlier: [{ name: "Ana", text: "Sí, hay vuelos baratos." }],
      addressed: false,
    });
    expect(message).toContain('<group_call group="Trip">');
    expect(message).toContain("voice call with the user and Ana, Leo.");
    expect(message).toContain("The group is for: Plan the trip to Cusco");
    expect(message).toContain("The user said: ¿Volamos el viernes?");
    expect(message).toContain("Ana said: Sí, hay vuelos baratos.");
    expect(message).toContain("no markdown");
    expect(message).toContain("reply with exactly PASS");
  });

  it("does not let a member the user named pass", () => {
    const message = callMessage({
      group: "Trip",
      others: [],
      said: "Leo, ¿y tú?",
      earlier: [],
      addressed: true,
    });
    expect(message).not.toContain("PASS");
    expect(message).toContain("voice call with the user.");
  });

  it("knows a pass, even dressed up or half streamed", () => {
    for (const text of ["PASS", " pass. ", "**PASS**", "`Pass`"])
      expect(isPass(text), text).toBe(true);
    expect(isPass("I'll pass on that, but Leo knows")).toBe(false);
    expect(couldBePass("PA")).toBe(true);
    expect(couldBePass("**")).toBe(true);
    expect(couldBePass("Pablo")).toBe(false);
    expect(couldBePass("Sí")).toBe(false);
  });
});

describe("turning a reply into speech", () => {
  it("drops markdown, links, code and emoji", () => {
    expect(
      speakable(
        "## Plan\n- **Fly** on [Friday](https://x.y/z) with `LATAM` 🚀 see https://a.b",
      ),
    ).toBe("Plan Fly on Friday with LATAM see");
  });

  it("hands over finished sentences while the reply streams", () => {
    const first = takeSentences("¡Hola! Soy Ana. Creo que", 0, false);
    expect(first.sentences).toEqual(["¡Hola!", "Soy Ana."]);
    const text = "¡Hola! Soy Ana. Creo que el viernes es mejor.";
    // The last sentence has no space after it yet: it may still go on.
    expect(takeSentences(text, first.next, false).sentences).toEqual([]);
    expect(takeSentences(text, first.next, true)).toEqual({
      sentences: ["Creo que el viernes es mejor."],
      next: text.length,
    });
  });

  it("keeps decimals, abbreviations and initials inside a sentence", () => {
    const text =
      "Cuesta 3.5 soles, dice el Sr. Pérez. J. R. Tolkien wrote it. ";
    expect(takeSentences(text, 0, false).sentences).toEqual([
      "Cuesta 3.5 soles, dice el Sr. Pérez.",
      "J. R. Tolkien wrote it.",
    ]);
  });

  it("splits on lines and skips code blocks, even unfinished ones", () => {
    const text =
      "Here it is:\n```ts\nconst a = 1. b\n```\nDone. Then ```py\nx = 1. y";
    const taken = takeSentences(text, 0, false);
    expect(taken.sentences).toEqual(["Here it is:", "Done.", "Then"]);
    expect(text.slice(taken.next).startsWith("```py")).toBe(true);
    expect(takeSentences(text, taken.next, true).sentences).toEqual([]);
  });

  it("cuts a very long sentence at a comma so speech can start", () => {
    const long = `${"palabra ".repeat(40)}, y luego más`;
    const taken = takeSentences(long, 0, false);
    expect(taken.sentences).toHaveLength(1);
    expect(long.slice(taken.next)).toBe(" y luego más");
  });

  it("tells Spanish from English and falls back when unsure", () => {
    expect(
      replyLanguage("Creo que el viernes es mejor para volar.", "en"),
    ).toBe("es");
    expect(
      replyLanguage("I think Friday is better for the flight.", "es"),
    ).toBe("en");
    expect(replyLanguage("¡Sí!", "en")).toBe("es");
    expect(replyLanguage("OK", "es")).toBe("es");
    expect(defaultCallLanguage("es-PE")).toBe("es");
    expect(defaultCallLanguage("en-US")).toBe("en");
  });

  it("gives each member its own voice, the same every call", () => {
    const ids = members.map((member) => member.id);
    const voices = memberVoices(ids, 10);
    expect(new Set(voices.values()).size).toBe(ids.length);
    expect(memberVoices(ids, 10)).toEqual(voices);
    for (const voice of voices.values()) expect(voice).toBeLessThan(10);
    expect([...memberVoices(ids, 1).values()]).toEqual([0, 0, 0, 0]);
  });
});

describe("reading a member's answer", () => {
  const block = (patch: Partial<Block>): Block => ({
    id: Math.random().toString(36),
    role: "assistant",
    text: "",
    ...patch,
  });

  it("joins its text after the call's message, around tool calls", () => {
    const blocks = [
      block({ role: "user", text: "earlier", appRequestId: "old" }),
      block({ text: "Old answer." }),
      block({ role: "user", text: "call", appRequestId: "call-1" }),
      block({ text: "Let me check." }),
      block({ text: "", tool: { title: "Search" } }),
      block({ text: "Flights are cheap.", streaming: true }),
      block({ role: "user", text: "next" }),
      block({ text: "Unrelated." }),
    ];
    expect(memberReply({ blocks }, "call-1")).toBe(
      "Let me check.\n\nFlights are cheap.",
    );
    expect(memberReply({ blocks }, "missing")).toBe("");
    expect(memberReply(undefined, "call-1")).toBe("");
  });
});

describe("the call's transcript", () => {
  const line = (patch: Partial<CallLine>): CallLine => ({
    id: Math.random().toString(36),
    who: "user",
    name: "You",
    text: "",
    status: "said",
    ...patch,
  });

  it("keeps what was said and leaves out passes", () => {
    const transcript = callTranscript("Trip", Date.UTC(2026, 10, 4, 22), [
      line({ text: "¿Volamos el viernes?" }),
      line({ who: "m-ana", name: "Ana", text: "Sí." }),
      line({ who: "m-leo", name: "Leo", status: "passed" }),
    ]);
    expect(transcript?.title).toMatch(/^Call with Trip, /);
    expect(transcript?.body).toContain("# Call with Trip");
    expect(transcript?.body).toContain("**You:** ¿Volamos el viernes?");
    expect(transcript?.body).toContain("**Ana:** Sí.");
    expect(transcript?.body).not.toContain("Leo");
  });

  it("saves nothing when no member spoke", () => {
    expect(callTranscript("Trip", 0, [line({ text: "Hola" })])).toBeUndefined();
  });
});
