/**
 * The topic a request's documents belong to, named by a small model: a few
 * words in the language the user wrote in, the same topic again when a
 * request carries on one already named. Folders in Documents are these
 * topics, so a chat's documents read by subject, not by its last message.
 */

/** What the model sees of one request. */
export type TopicRequest = {
  /** The turn the documents hang from. */
  turnId: string;
  request: string;
  /** The start of the bot's reply. */
  reply: string;
  /** Titles of the documents and file names it made. */
  documents: readonly string[];
};

const TOPIC_MAX = 60;
const EXCERPT = 600;

const excerpt = (text: string, max = EXCERPT) => {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
};

/** The prompt that names one request's topic, given the topics so far. */
export function topicPrompt(
  request: TopicRequest,
  existing: readonly string[],
): string {
  const known = existing.length
    ? existing.map((topic) => `- ${topic}`).join("\n")
    : "(none yet)";
  return [
    "You name folders for a user's research documents. Reply with only the folder name, nothing else.",
    "",
    "Rules:",
    "- 2 to 6 words naming the subject (for example \"Diseño de pozas de sedimentación\" or \"HEC-HMS model automation\"), not the action or the request.",
    "- Write it in the language of the user's request. If the request is only a short instruction such as \"continue\", use the language of the reply.",
    "- If this request continues or belongs to one of the existing topics, reply with that topic exactly as written.",
    "- Sentence case, no quotes, no final period, no emoji.",
    "",
    "Existing topics in this chat:",
    known,
    "",
    `User's request: ${excerpt(request.request) || "(none)"}`,
    `Start of the reply: ${excerpt(request.reply) || "(none)"}`,
    `Documents made: ${request.documents.length ? request.documents.map((title) => excerpt(title, 120)).join("; ") : "(none)"}`,
  ].join("\n");
}

/**
 * The topic in a model's answer: its first line, without quotes, labels or a
 * final period, cut to a folder's length. An existing topic that differs only
 * in case or spacing is that topic. Null when nothing usable came back.
 */
export function parseTopic(
  output: string,
  existing: readonly string[],
): string | null {
  const line =
    output
      .split("\n")
      .map((part) => part.trim())
      .find(Boolean) ?? "";
  const topic = line
    .replace(/^(folder( name)?|topic|nombre|tema)\s*:\s*/i, "")
    .replace(/^["'`*«“]+|["'`*»”]+$/g, "")
    .replace(/[.。]+$/, "")
    .replace(/\s+/g, " ")
    .trim();
  if (!topic || topic.length > TOPIC_MAX * 2) return null;
  const key = topic.toLowerCase();
  const same = existing.find((entry) => entry.toLowerCase() === key);
  if (same) return same;
  return topic.length > TOPIC_MAX
    ? `${topic.slice(0, TOPIC_MAX - 1).trimEnd()}…`
    : topic;
}

const KEY = "monocode.documentTopics";
const EVENT = "monocode:document-topics";
const MAX_ENTRIES = 2_000;
let cache: Readonly<Record<string, string>> | null = null;

function read(): Readonly<Record<string, string>> {
  if (cache) return cache;
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    cache =
      parsed && typeof parsed === "object" && !Array.isArray(parsed)
        ? Object.fromEntries(
            Object.entries(parsed).filter(
              (entry): entry is [string, string] =>
                typeof entry[1] === "string" && !!entry[1].trim(),
            ),
          )
        : {};
  } catch {
    cache = {};
  }
  return cache;
}

/** Each turn's topic, by turn id, for useSyncExternalStore. */
export function documentTopicsSnapshot(): Readonly<Record<string, string>> {
  return read();
}

export function setDocumentTopic(turnId: string, topic: string) {
  const current = read();
  if (current[turnId] === topic) return;
  const entries = Object.entries({ ...current, [turnId]: topic });
  cache = Object.fromEntries(entries.slice(-MAX_ENTRIES));
  try {
    localStorage.setItem(KEY, JSON.stringify(cache));
  } catch {
    // no storage: the names last for this window
  }
  if (typeof window !== "undefined") window.dispatchEvent(new Event(EVENT));
}

export function subscribeDocumentTopics(onChange: () => void): () => void {
  if (typeof window === "undefined") return () => {};
  const onStorage = (event: StorageEvent) => {
    if (event.key !== KEY) return;
    cache = null;
    onChange();
  };
  window.addEventListener(EVENT, onChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(EVENT, onChange);
    window.removeEventListener("storage", onStorage);
  };
}
