/** What personal bots post on their own (a habit's report, a digest, an
 * alert), gathered into one feed so the user reads the news in one place. */
export type BotNewsItem = {
  id: string;
  monoId: string;
  botName: string;
  /** What posted it, e.g. the habit's name. */
  source: string;
  text: string;
  at: number;
  seen: boolean;
};

const KEY = "monocode.botNews";
const EVENT = "monocode:bot-news";
const MAX_ITEMS = 200;
const MAX_TEXT = 2_000;

let cache: BotNewsItem[] | null = null;

function read(): BotNewsItem[] {
  if (cache) return cache;
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(KEY) ?? "[]");
    cache = Array.isArray(parsed)
      ? parsed.filter(
          (item): item is BotNewsItem =>
            !!item &&
            typeof item.id === "string" &&
            typeof item.monoId === "string" &&
            typeof item.text === "string" &&
            typeof item.at === "number",
        )
      : [];
  } catch {
    cache = [];
  }
  return cache;
}

function write(items: BotNewsItem[]) {
  cache = items;
  try {
    localStorage.setItem(KEY, JSON.stringify(items));
  } catch {
    // quota or private mode: the feed lives for this window only
  }
  if (typeof window !== "undefined") window.dispatchEvent(new Event(EVENT));
}

/** Newest first. */
export function listBotNews(): BotNewsItem[] {
  return read();
}

export function addBotNews(
  item: Omit<BotNewsItem, "id" | "at" | "seen"> & { at?: number },
): BotNewsItem {
  const entry: BotNewsItem = {
    id: crypto.randomUUID(),
    monoId: item.monoId,
    botName: item.botName,
    source: item.source,
    text: item.text.slice(0, MAX_TEXT),
    at: item.at ?? Date.now(),
    seen: false,
  };
  write([entry, ...read()].slice(0, MAX_ITEMS));
  return entry;
}

export function markBotNewsSeen() {
  const items = read();
  if (items.every((item) => item.seen)) return;
  write(items.map((item) => (item.seen ? item : { ...item, seen: true })));
}

export function removeBotNewsFor(monoId: string) {
  write(read().filter((item) => item.monoId !== monoId));
}

export function hasUnseenBotNews(): boolean {
  return read().some((item) => !item.seen);
}

export function subscribeBotNews(onChange: () => void): () => void {
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

/** A snapshot that changes only when the feed does, for useSyncExternalStore. */
export function botNewsSnapshot(): BotNewsItem[] {
  return read();
}
