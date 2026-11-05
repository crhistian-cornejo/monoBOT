import { isEditTool } from "../../integrations/harness/core/preview";
import { CONTINUE_PROMPT } from "../sessions/model/inFlight";
import type { Block } from "../sessions/model/session";
import { finalFirst, type ArtifactCard } from "./artifacts";
import type { TopicRequest } from "./documentTopics";

/**
 * A chat's documents kept together like a folder: one per topic, named by a
 * small model (see documentTopics), or per request until it is named. It
 * holds the final report ahead of the papers and drafts gathered on the way,
 * and the files the bot wrote (PDF, Word, Excel, slides).
 */
export type DocumentGroup = {
  id: string;
  title: string;
  /** When its latest request began. */
  at?: number;
  cards: ArtifactCard[];
  /** Absolute paths of the files the bot wrote or pointed to. */
  files: string[];
};

/** One request and what it left behind. */
type DocumentTurn = {
  id: string;
  request: Block;
  reply: string;
  at?: number;
  cards: ArtifactCard[];
  files: string[];
  /** A "continue" or an app prompt: it belongs with the request before it. */
  carriesOn: boolean;
};

const TITLE_MAX = 90;

/** The kinds of file a folder lists beside its documents. */
const FILE_EXTENSIONS = "pdf|docx?|xlsx?|xlsm|pptx?|csv";
// A path in a Markdown link's angle brackets or in backticks may hold spaces.
const QUOTED_PATH = new RegExp(
  `(?:<|\`)((?:[A-Za-z]:[\\\\/]|/)[^<>\`\\n]+?\\.(?:${FILE_EXTENSIONS}))(?:>|\`)`,
  "gi",
);
const BARE_PATH = new RegExp(
  `(?:^|[\\s("'\\[])((?:[A-Za-z]:[\\\\/]|/)[^\\s<>"'\`()\\[\\]|*?]+\\.(?:${FILE_EXTENSIONS}))(?=$|[\\s)"'\\].,;:])`,
  "gim",
);
const FILE_PATH = new RegExp(`\\.(?:${FILE_EXTENSIONS})$`, "i");

/** Absolute paths of files with those extensions named in a text. */
export function filePathsIn(text: string): string[] {
  const found = new Set<string>();
  for (const pattern of [QUOTED_PATH, BARE_PATH])
    for (const match of text.matchAll(pattern))
      // A Markdown link writes a Windows path as </C:/...>.
      found.add(match[1].trim().replace(/^\/([A-Za-z]:[\\/])/, "$1"));
  return [...found];
}

/** The files a turn wrote, ran a command on or named in its reply. */
function turnFiles(blocks: readonly Block[]): string[] {
  const files = new Set<string>();
  for (const block of blocks) {
    const tool = block.role === "tool" ? block.tool : undefined;
    if (tool && ["completed", "success"].includes(tool.status ?? "")) {
      if (isEditTool(tool.kind, tool.title, tool.preview)) {
        for (const path of tool.paths ?? [])
          if (FILE_PATH.test(path)) files.add(path);
        if (tool.preview?.path && FILE_PATH.test(tool.preview.path))
          files.add(tool.preview.path);
      }
      for (const path of filePathsIn(`${tool.title ?? ""}\n${tool.detail ?? ""}`))
        files.add(path);
    }
    if (block.role === "assistant")
      for (const path of filePathsIn(block.text)) files.add(path);
  }
  return [...files];
}

function firstLine(text: string): string {
  return (
    text
      .split("\n")
      .map((part) => part.trim())
      .find(Boolean) ?? ""
  );
}

/** The request's first line, short enough for a folder's name. */
function requestTitle(block: Block): string {
  if (block.monoHabit) return block.monoHabit.name;
  const line = firstLine(block.text);
  if (!line) return "Documents";
  return line.length > TITLE_MAX
    ? `${line.slice(0, TITLE_MAX - 1).trimEnd()}…`
    : line;
}

/**
 * Each request with what it left behind, oldest first: the documents whose
 * card hangs from it (a document revised later moves to the later request, as
 * the chat shows it) and the files it made. A habit's report is a request of
 * its own.
 */
function documentTurns(blocks: readonly Block[]): DocumentTurn[] {
  const owner = new Map<string, string>();
  for (const block of blocks)
    for (const card of block.artifactCards ?? []) owner.set(card.id, block.id);
  const segments: Block[][] = [];
  for (const block of blocks) {
    if (block.role === "user" || block.monoHabit) segments.push([block]);
    else segments[segments.length - 1]?.push(block);
  }
  const turns: DocumentTurn[] = [];
  for (const segment of segments) {
    const request = segment[0];
    const cards = segment.flatMap((block) =>
      (block.artifactCards ?? []).filter(
        (card) => owner.get(card.id) === block.id,
      ),
    );
    const files = turnFiles(request.monoHabit ? segment : segment.slice(1));
    if (!cards.length && !files.length) continue;
    const reply = request.monoHabit
      ? request.text
      : (segment
          .slice(1)
          .reverse()
          .find((block) => block.role === "assistant" && block.text.trim())
          ?.text ?? "");
    const at = request.monoHabit?.at ?? request.startedAt ?? request.sentAt;
    turns.push({
      id: request.id,
      request,
      reply,
      ...(at !== undefined ? { at } : {}),
      cards,
      files,
      carriesOn:
        !request.monoHabit &&
        (request.text.trim() === CONTINUE_PROMPT ||
          !!request.internal ||
          !!request.monocode),
    });
  }
  return turns;
}

/**
 * A chat's documents as folders, newest first: by topic once a request is
 * named, by request until then. A "continue" stays with the request before
 * it. Files the bot wrote sit beside the documents.
 */
export function documentGroups(
  blocks: readonly Block[],
  topics: Readonly<Record<string, string>> = {},
): DocumentGroup[] {
  const groups = new Map<string, DocumentGroup & { order: number }>();
  let previousKey: string | null = null;
  documentTurns(blocks).forEach((turn, order) => {
    const topic = turn.request.monoHabit
      ? turn.request.monoHabit.name
      : topics[turn.id]?.trim();
    const key: string = topic
      ? `topic:${topic.toLowerCase()}`
      : turn.carriesOn && previousKey
        ? previousKey
        : `turn:${turn.id}`;
    previousKey = key;
    const group = groups.get(key);
    if (!group) {
      groups.set(key, {
        id: key,
        title: topic || requestTitle(turn.request),
        ...(turn.at !== undefined ? { at: turn.at } : {}),
        cards: [...turn.cards],
        files: [...turn.files],
        order,
      });
      return;
    }
    group.cards = [
      ...group.cards.filter((card) => !turn.cards.some((next) => next.id === card.id)),
      ...turn.cards,
    ];
    group.files = [...new Set([...group.files, ...turn.files])];
    if (turn.at !== undefined) group.at = turn.at;
    group.order = order;
  });
  return [...groups.values()]
    .sort((a, b) => b.order - a.order)
    .map(({ order: _order, ...group }) => ({
      ...group,
      cards: finalFirst(group.cards),
    }));
}

/**
 * The requests whose documents still need a topic, oldest first, each with
 * what the model is shown. A habit's documents take the habit's name, and a
 * "continue" goes with the request before it, so neither is asked about.
 */
export function pendingTopicTurns(
  blocks: readonly Block[],
  topics: Readonly<Record<string, string>>,
): TopicRequest[] {
  return documentTurns(blocks)
    .filter(
      (turn) => !turn.request.monoHabit && !turn.carriesOn && !topics[turn.id],
    )
    .map((turn) => ({
      turnId: turn.id,
      request: turn.request.text,
      reply: turn.reply,
      documents: [
        ...turn.cards.map((card) => card.title),
        ...turn.files.map((path) => path.split(/[\\/]/).pop() ?? path),
      ],
    }));
}

/** The topics named so far in a chat, oldest first, without repeats. */
export function chatTopics(
  blocks: readonly Block[],
  topics: Readonly<Record<string, string>>,
): string[] {
  const seen = new Map<string, string>();
  for (const turn of documentTurns(blocks)) {
    const topic = topics[turn.id];
    if (topic && !seen.has(topic.toLowerCase()))
      seen.set(topic.toLowerCase(), topic);
  }
  return [...seen.values()];
}

/** "Today", "Yesterday", or the date, for a folder's day. */
export function groupDay(at: number | undefined, now = Date.now()): string {
  if (at === undefined) return "Earlier";
  const day = new Date(at);
  const today = new Date(now);
  const start = (date: Date) =>
    new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  const days = Math.round((start(today) - start(day)) / 86_400_000);
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  return day.toLocaleDateString([], {
    month: "short",
    day: "numeric",
    ...(day.getFullYear() !== today.getFullYear() ? { year: "numeric" } : {}),
  });
}

const STARS_KEY = "monocode.documentStars";
const STARS_EVENT = "monocode:document-stars";
let stars: ReadonlySet<string> | null = null;

function readStars(): ReadonlySet<string> {
  if (stars) return stars;
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(STARS_KEY) ?? "[]");
    stars = new Set(
      Array.isArray(parsed)
        ? parsed.filter((id): id is string => typeof id === "string")
        : [],
    );
  } catch {
    stars = new Set();
  }
  return stars;
}

/** Documents the user starred to find again, for useSyncExternalStore. */
export function documentStarsSnapshot(): ReadonlySet<string> {
  return readStars();
}

export function toggleDocumentStar(id: string) {
  const next = new Set(readStars());
  if (next.has(id)) next.delete(id);
  else next.add(id);
  stars = next;
  try {
    localStorage.setItem(STARS_KEY, JSON.stringify([...next]));
  } catch {
    // no storage: the star lasts for this window
  }
  window.dispatchEvent(new Event(STARS_EVENT));
}

export function subscribeDocumentStars(onChange: () => void): () => void {
  const onStorage = (event: StorageEvent) => {
    if (event.key !== STARS_KEY) return;
    stars = null;
    onChange();
  };
  window.addEventListener(STARS_EVENT, onChange);
  window.addEventListener("storage", onStorage);
  return () => {
    window.removeEventListener(STARS_EVENT, onChange);
    window.removeEventListener("storage", onStorage);
  };
}
