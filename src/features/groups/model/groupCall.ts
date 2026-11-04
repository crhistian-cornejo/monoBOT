import type { Session } from "../../sessions/model/session";

/**
 * A voice call with a group: the user talks, and the members answer out loud
 * one after another, each from its own conversation, as group.ask does in
 * text. The leader chat is left out so the first voice comes one Claude turn
 * after the user stops, not two.
 */
export type CallMember = { id: string; name: string };

export type CallLanguage = "es" | "en";

export type CallLine = {
  id: string;
  /** "user", or the member's Mono id. */
  who: string;
  name: string;
  /** What was said, as plain speech. */
  text: string;
  status: "said" | "thinking" | "passed" | "busy" | "failed" | "stopped";
  note?: string;
};

/** Most members that speak to one thing the user says, unless named. */
export const CALL_MAX_SPEAKERS = 4;
/**
 * Silence after the detector closes a phrase before the turn passes. The
 * detector already waits 0.45 s, so the user has paused about a second.
 */
export const CALL_TURN_GAP_MS = 600;

/** Lowercase, without accents, so "Sofía" and "sofia" are one name. */
export function foldName(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase();
}

function words(text: string): string {
  return ` ${foldName(text)
    .replace(/[^\p{L}\p{N}]+/gu, " ")
    .trim()} `;
}

/** Members the user named, in the order they were named. */
export function addressedMembers(
  members: readonly CallMember[],
  said: string,
): string[] {
  const heard = words(said);
  return members
    .flatMap((member) => {
      const name = words(member.name);
      const at = name.trim() ? heard.indexOf(name) : -1;
      return at >= 0 ? [{ id: member.id, at }] : [];
    })
    .sort((a, b) => a.at - b.at)
    .map((entry) => entry.id);
}

/**
 * Who answers, in order: the members the user named, or else everyone,
 * starting after the member who went first last time so no one always leads.
 */
export function callTurnOrder(
  members: readonly CallMember[],
  said: string,
  previousFirst?: string,
): { order: string[]; addressed: boolean } {
  const named = addressedMembers(members, said);
  if (named.length) return { order: named, addressed: true };
  const start = previousFirst
    ? (members.findIndex((member) => member.id === previousFirst) + 1) %
      Math.max(1, members.length)
    : 0;
  return {
    order: [...members.slice(start), ...members.slice(0, start)].map(
      (member) => member.id,
    ),
    addressed: false,
  };
}

/** What a member is told when its turn on the call comes. */
export function callMessage(input: {
  group: string;
  goal?: string;
  /** Everyone else on the call, by name. */
  others: readonly string[];
  said: string;
  /** Who already answered this, in order. */
  earlier: readonly { name: string; text: string }[];
  /** The user named this member: it answers rather than passing. */
  addressed: boolean;
}): string {
  const company = input.others.length ? ` and ${input.others.join(", ")}` : "";
  const lines = [
    `You are on a voice call with the user${company}.`,
    ...(input.goal ? [`The group is for: ${input.goal}`] : []),
    `The user said: ${input.said.trim()}`,
    ...input.earlier.map((entry) => `${entry.name} said: ${entry.text}`),
  ];
  return `<group_call group="${input.group}">
${lines.join("\n")}
</group_call>
Answer out loud as yourself: one to three short sentences in the language the user spoke, as plain speech with no markdown, lists, code or links.${
    input.addressed
      ? ""
      : " Add something new rather than repeat the others. If you have nothing to add, reply with exactly PASS."
  }`;
}

/** The member had nothing to add. */
export function isPass(text: string): boolean {
  return /^[\s*_`"'.!¡?¿-]*pass[\s*_`"'.!?-]*$/i.test(text);
}

/** A reply still streaming in that may turn out to be PASS, so not spoken. */
export function couldBePass(text: string): boolean {
  const letters = text.replace(/[^\p{L}]/gu, "").toLowerCase();
  return letters.length <= 4 && "pass".startsWith(letters);
}

/** A sentence as the voice should read it: no markdown, links or emoji. */
export function speakable(text: string): string {
  return text
    .replace(/!\[[^\]]*\]\([^)]*\)/g, " ")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/`([^`]*)`/g, "$1")
    .replace(/<[^>]+>/g, " ")
    .replace(/https?:\/\/\S+/g, " ")
    .replace(/^\s*(#{1,6}|>|[-*+•]|\d+[.)])\s+/gm, "")
    .replace(/[*_~|`#]/g, "")
    .replace(/\p{Extended_Pictographic}|️/gu, "")
    .replace(/\s+/g, " ")
    .trim();
}

const ABBREVIATIONS = new Set(
  "sr sra srta dr dra ud uds etc ej vs mr mrs ms st no pag aprox approx eg ie e g i".split(
    " ",
  ),
);

function endsWithAbbreviation(segment: string): boolean {
  const last = /([\p{L}]+)[.]$/u.exec(segment.trimEnd());
  if (!last) return false;
  const word = last[1];
  // An initial, like the J in "J. R.", is not the end of a sentence.
  return (
    ABBREVIATIONS.has(word.toLowerCase()) ||
    (word.length === 1 && word === word.toUpperCase())
  );
}

/** Longest a sentence runs before it is cut at a comma, to start speaking. */
const LONG_SENTENCE = 280;

/**
 * The sentences a streaming reply has finished since `from`, ready to speak,
 * and where the unfinished rest starts. Code blocks are skipped. With `done`
 * the rest counts as finished too.
 */
export function takeSentences(
  text: string,
  from: number,
  done: boolean,
): { sentences: string[]; next: number } {
  const sentences: string[] = [];
  const push = (segment: string) => {
    const spoken = speakable(segment);
    if (/[\p{L}\p{N}]/u.test(spoken)) sentences.push(spoken);
  };
  let start = Math.min(Math.max(0, from), text.length);
  let index = start;
  while (index < text.length) {
    if (text.startsWith("```", index)) {
      push(text.slice(start, index));
      const close = text.indexOf("```", index + 3);
      if (close < 0) {
        // An open code block: nothing after it is speech until it closes.
        start = done ? text.length : index;
        return { sentences, next: start };
      }
      index = close + 3;
      start = index;
      continue;
    }
    const char = text[index];
    if (char === "\n") {
      push(text.slice(start, index));
      start = index + 1;
      index = start;
      continue;
    }
    if (/[.!?…]/.test(char)) {
      let end = index + 1;
      while (end < text.length && /[.!?…"'”’)\]»]/.test(text[end])) end++;
      if (end >= text.length) break;
      if (
        /\s/.test(text[end]) &&
        !endsWithAbbreviation(text.slice(start, end))
      ) {
        push(text.slice(start, end));
        start = end;
      }
      index = end;
      continue;
    }
    if (
      char === "," &&
      index - start > LONG_SENTENCE &&
      /\s/.test(text[index + 1] ?? "")
    ) {
      push(text.slice(start, index + 1));
      start = index + 1;
    }
    index++;
  }
  if (done && start < text.length) {
    push(text.slice(start));
    start = text.length;
  }
  return { sentences, next: start };
}

const SPANISH = new Set(
  "el la los las que de del y en un una unos por para con es está están son pero muy más lo se su sus al como también hola sí gracias porque cuando donde dónde qué cómo yo tú usted nosotros esto eso esta este hay ser tiene tengo puedo puede creo mejor bueno vamos ahora".split(
    " ",
  ),
);
const ENGLISH = new Set(
  "the and is are you to of it that this with for what i we be have has not but so can will just yes hello thanks because when where how my your there think good let now our they them".split(
    " ",
  ),
);

/** Spanish or English, by its common words; `fallback` when unsure. */
export function replyLanguage(
  text: string,
  fallback: CallLanguage,
): CallLanguage {
  let spanish = (text.match(/[ñ¿¡áéíóú]/gi) ?? []).length * 2;
  let english = 0;
  for (const word of text.toLowerCase().split(/[^\p{L}']+/u)) {
    if (SPANISH.has(word)) spanish++;
    if (ENGLISH.has(word)) english++;
  }
  return spanish > english ? "es" : english > spanish ? "en" : fallback;
}

/** The language the app is used in, for replies too short to tell. */
export function defaultCallLanguage(locale: string): CallLanguage {
  return locale.toLowerCase().startsWith("es") ? "es" : "en";
}

function hash(text: string): number {
  let value = 0x811c9dc5;
  for (let index = 0; index < text.length; index++) {
    value ^= text.charCodeAt(index);
    value = Math.imul(value, 0x01000193);
  }
  return value >>> 0;
}

/**
 * A voice for each member: the same one every call, and different from the
 * others' while there are voices to go round.
 */
export function memberVoices(
  ids: readonly string[],
  speakers: number,
): Map<string, number> {
  const count = Math.max(1, Math.floor(speakers));
  const taken = new Set<number>();
  const voices = new Map<string, number>();
  for (const id of ids) {
    let voice = hash(id) % count;
    for (let step = 0; step < count && taken.has(voice); step++)
      voice = (voice + 1) % count;
    taken.add(voice);
    voices.set(id, voice);
  }
  return voices;
}

/**
 * What a member has answered so far to the call's message `requestId`: its
 * text after that message, joined across tool calls.
 */
export function memberReply(
  session: Pick<Session, "blocks"> | undefined,
  requestId: string,
): string {
  if (!session) return "";
  const start = session.blocks.findIndex(
    (block) => block.role === "user" && block.appRequestId === requestId,
  );
  if (start < 0) return "";
  const parts: string[] = [];
  for (const block of session.blocks.slice(start + 1)) {
    if (block.role === "user") break;
    if (
      block.role === "assistant" &&
      !block.tool &&
      !block.internal &&
      block.text
    )
      parts.push(block.text);
  }
  return parts.join("\n\n");
}

/** The call as a document for the group's Documents tab. */
export function callTranscript(
  group: string,
  startedAt: number,
  lines: readonly CallLine[],
): { title: string; body: string } | undefined {
  const spoken = lines.filter((line) => line.status === "said" && line.text);
  if (!spoken.some((line) => line.who !== "user")) return undefined;
  const when = new Date(startedAt);
  const date = when.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  return {
    title: `Call with ${group}, ${date}`,
    body: [
      `# Call with ${group}`,
      "",
      `_${date}_`,
      "",
      ...spoken.map((line) => `**${line.name}:** ${line.text}`),
    ]
      .join("\n\n")
      .replace(/\n{3,}/g, "\n\n"),
  };
}
