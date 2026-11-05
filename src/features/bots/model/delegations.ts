/**
 * What one agent asked another with agents.message: the Principal handing a
 * bot its part, mostly. The Principal's Bots panel lists them by bot, with
 * the bot's live state beside, so the user sees who is on what.
 */
export type Delegation = {
  id: string;
  /** The Mono that asked. */
  from: string;
  /** The Mono it asked. */
  to: string;
  message: string;
  at: number;
};

const KEY = "monocode.delegations.v1";
/** Enough for weeks of work; older ones drop off. */
const MAX = 200;

let cache: Delegation[] | null = null;
const listeners = new Set<() => void>();

function parse(text: string | null): Delegation[] {
  try {
    const value = JSON.parse(text ?? "[]");
    return Array.isArray(value)
      ? value.filter(
          (entry): entry is Delegation =>
            !!entry &&
            typeof entry.id === "string" &&
            typeof entry.from === "string" &&
            typeof entry.to === "string" &&
            typeof entry.message === "string" &&
            typeof entry.at === "number",
        )
      : [];
  } catch {
    return [];
  }
}

export function delegationsSnapshot(): Delegation[] {
  if (cache) return cache;
  try {
    cache = parse(localStorage.getItem(KEY));
  } catch {
    cache = [];
  }
  return cache;
}

export function subscribeDelegations(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Newest first. */
export function recordDelegation(entry: Omit<Delegation, "id">): void {
  const next = [
    { ...entry, id: crypto.randomUUID() },
    ...delegationsSnapshot(),
  ].slice(0, MAX);
  cache = next;
  try {
    localStorage.setItem(KEY, JSON.stringify(next));
  } catch {
    // kept for this window
  }
  listeners.forEach((listener) => listener());
}

/** The latest request to each agent, newest first: one row per agent. */
export function latestByAgent(
  delegations: readonly Delegation[],
  from: string,
): Delegation[] {
  const seen = new Set<string>();
  return delegations.filter((entry) => {
    if (entry.from !== from || seen.has(entry.to)) return false;
    seen.add(entry.to);
    return true;
  });
}

/** Clears the in-memory copy; for tests. */
export function resetDelegationsForTest() {
  cache = null;
}
