import type { Block } from "../../sessions/model/session";

/**
 * A group learns as it goes: once an exchange in its chat settles, a small
 * model reads it and the facts worth keeping join the project's memory,
 * whether or not the lead thought to save them. The members read that
 * memory with every message the group sends them.
 */

const KEY = "monocode.groupLearned";
const FACTS_MAX = 5;
const FACT_MAX_CHARS = 300;
const EXCHANGE_MAX_CHARS = 12_000;

/** The last block each group's chat was learned up to, by session id. */
function learnedMarks(): Record<string, string> {
  try {
    const value = JSON.parse(localStorage.getItem(KEY) ?? "{}") as unknown;
    return value && typeof value === "object"
      ? (value as Record<string, string>)
      : {};
  } catch {
    return {};
  }
}

export function learnedUpTo(sessionId: string): string | undefined {
  return learnedMarks()[sessionId];
}

export function markLearned(sessionId: string, blockId: string) {
  try {
    localStorage.setItem(
      KEY,
      JSON.stringify({ ...learnedMarks(), [sessionId]: blockId }),
    );
  } catch {
    // Without the mark the exchange is read again, and its facts dedupe.
  }
}

export type GroupExchange = {
  /** The block it ends on, the mark for next time. */
  lastId: string;
  lines: string[];
};

/**
 * What was said since the chat was last learned from: the user's messages,
 * the members' answers and the lead's replies. Undefined until the user has
 * written something new and someone answered it.
 */
export function pendingExchange(
  blocks: readonly Block[],
  mark: string | undefined,
  leadName: string,
): GroupExchange | undefined {
  const start = mark ? blocks.findIndex((block) => block.id === mark) + 1 : 0;
  const lines: string[] = [];
  let userWrote = false;
  let answered = false;
  let lastId: string | undefined;
  for (const block of blocks.slice(Math.max(0, start))) {
    if (block.internal || block.draft || block.tool || !block.text.trim())
      continue;
    const text = block.text.replace(/\s+/g, " ").trim();
    if (block.role === "user") {
      userWrote = true;
      lines.push(`User: ${text}`);
    } else if (block.role === "assistant") {
      answered = answered || userWrote;
      const who = block.groupMember?.name ?? leadName;
      lines.push(`${who}: ${text}`);
    } else continue;
    lastId = block.id;
  }
  if (!userWrote || !answered || !lastId) return undefined;
  let budget = EXCHANGE_MAX_CHARS;
  const kept: string[] = [];
  for (const line of [...lines].reverse()) {
    if (budget <= 0) break;
    kept.unshift(line.slice(0, budget));
    budget -= line.length;
  }
  return { lastId, lines: kept };
}

export function learningPrompt(
  groupName: string,
  goal: string | undefined,
  memory: string,
  exchange: GroupExchange,
): string {
  return [
    `You keep the memory of the project "${groupName}"${goal ? `, which is for: ${goal}` : ""}. Below is what it already remembers and the latest exchange in its group chat.`,
    "List the facts from the exchange that the project will need again: decisions, the user's preferences and corrections, deadlines, people and their roles, names of files, folders and links that matter, and what was agreed to do next. Leave out greetings, what is already remembered, one-off answers and anything that only held for this exchange. Never include passwords, tokens or keys.",
    `Reply with at most ${FACTS_MAX} lines, each "- " and one short self-contained fact in the language of the exchange. If there is nothing worth keeping, reply exactly NONE.`,
    "",
    "<memory>",
    memory.trim() || "(empty)",
    "</memory>",
    "",
    "<exchange>",
    ...exchange.lines,
    "</exchange>",
  ].join("\n");
}

/** The facts in the model's answer, without those already remembered. */
export function parseLearned(output: string, memory: string): string[] {
  if (/^\s*NONE\s*$/i.test(output)) return [];
  const known = memory.toLowerCase();
  const seen = new Set<string>();
  return output
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /^[-*•]\s+/.test(line))
    .map((line) => line.replace(/^[-*•]\s+/, "").trim())
    .filter((fact) => {
      const key = fact.toLowerCase();
      if (!fact || fact.length > FACT_MAX_CHARS || /^none$/i.test(fact))
        return false;
      if (known.includes(key) || seen.has(key)) return false;
      seen.add(key);
      return true;
    })
    .slice(0, FACTS_MAX);
}
