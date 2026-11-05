import type { Block } from "../sessions/model/session";
import { finalFirst, type ArtifactCard } from "./artifacts";

/**
 * The documents one request produced, kept together like a folder: what the
 * user asked (or the habit that ran), when, and the final report ahead of the
 * papers and drafts gathered on the way.
 */
export type DocumentGroup = {
  /** The turn the documents hang from. */
  id: string;
  title: string;
  at?: number;
  cards: ArtifactCard[];
};

const TITLE_MAX = 90;

/** The request's first line, short enough for a folder's name. */
function groupTitle(block: Block): string {
  if (block.monoHabit) return block.monoHabit.name;
  const line =
    block.text
      .split("\n")
      .map((part) => part.trim())
      .find(Boolean) ?? "";
  if (!line) return "Documents";
  return line.length > TITLE_MAX
    ? `${line.slice(0, TITLE_MAX - 1).trimEnd()}…`
    : line;
}

function groupTime(block: Block): number | undefined {
  return block.monoHabit?.at ?? block.startedAt ?? block.sentAt;
}

/**
 * A chat's documents by the request that made them, newest request first. A
 * document revised under a later request moves to that one, as it is shown
 * there in the chat.
 */
export function documentGroups(blocks: readonly Block[]): DocumentGroup[] {
  const owner = new Map<string, string>();
  for (const block of blocks)
    for (const card of block.artifactCards ?? []) owner.set(card.id, block.id);
  const groups: DocumentGroup[] = [];
  for (const block of blocks) {
    const cards = (block.artifactCards ?? []).filter(
      (card) => owner.get(card.id) === block.id,
    );
    if (!cards.length) continue;
    const at = groupTime(block);
    groups.push({
      id: block.id,
      title: groupTitle(block),
      ...(at !== undefined ? { at } : {}),
      cards: finalFirst(cards),
    });
  }
  return groups.reverse();
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
