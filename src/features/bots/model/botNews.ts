import type { ReportStatus } from "../../monos/model/monoHabits";

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
  status?: ReportStatus;
};

/** The same bot saying the same thing within a day is not news again. */
const DUPLICATE_WINDOW_MS = 24 * 60 * 60 * 1000;
/** Routine posts alert at most this often; attention and blocked always do. */
export const NEWS_ALERT_INTERVAL_MS = 60 * 60 * 1000;
const ALERT_KEY = "monocode.botNews.lastAlert";

const normalized = (text: string) => text.replace(/\s+/g, " ").trim().toLowerCase();

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

/** Adds a post to the feed; a repeat of a recent one is dropped (null). */
export function addBotNews(
  item: Omit<BotNewsItem, "id" | "at" | "seen"> & { at?: number },
): BotNewsItem | null {
  const at = item.at ?? Date.now();
  const text = item.text.slice(0, MAX_TEXT);
  const key = normalized(text);
  const repeat = read().some(
    (entry) =>
      entry.monoId === item.monoId &&
      at - entry.at < DUPLICATE_WINDOW_MS &&
      normalized(entry.text) === key,
  );
  if (repeat) return null;
  const entry: BotNewsItem = {
    id: crypto.randomUUID(),
    monoId: item.monoId,
    botName: item.botName,
    source: item.source,
    text,
    at,
    seen: false,
    ...(item.status ? { status: item.status } : {}),
  };
  write([entry, ...read()].slice(0, MAX_ITEMS));
  return entry;
}

/**
 * Whether a post should raise an OS alert now. Attention and blocked always
 * do; the rest at most once an hour, the feed holding what came between.
 */
export function shouldAlertNews(
  status: ReportStatus | undefined,
  now = Date.now(),
): boolean {
  if (status === "needs_attention" || status === "blocked") return true;
  let last = 0;
  try {
    last = Number(localStorage.getItem(ALERT_KEY)) || 0;
  } catch {
    // no storage: alert
  }
  if (now - last < NEWS_ALERT_INTERVAL_MS) return false;
  try {
    localStorage.setItem(ALERT_KEY, String(now));
  } catch {
    // alerts again next time
  }
  return true;
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
