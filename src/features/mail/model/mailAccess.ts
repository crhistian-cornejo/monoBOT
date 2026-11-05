/**
 * Which agents may read each mail account. An account the user connected in
 * the Email tab is open to every agent, as before. One a bot asked to connect
 * (the partner's, to track shared spending) is kept for that bot alone until
 * the user opens it to all. The user always sees every account in the Inbox.
 * Pure but for the localStorage wrappers at the end.
 */

/** Address (lowercase) → the ids of the Monos that may read it. */
export type MailAccess = Record<string, string[]>;

const KEY = "monocode.mail.private";

function key(address: string): string {
  return address.trim().toLowerCase();
}

/** Whether the Mono `reader` may read `address`; no reader, only open accounts. */
export function mayRead(
  access: MailAccess,
  address: string,
  reader: string | undefined,
): boolean {
  const only = access[key(address)];
  return !only || (reader !== undefined && only.includes(reader));
}

/** Keeps `address` for `reader` too; an open account becomes reader's alone. */
export function keepFor(
  access: MailAccess,
  address: string,
  reader: string,
): MailAccess {
  const only = access[key(address)] ?? [];
  return only.includes(reader)
    ? access
    : { ...access, [key(address)]: [...only, reader] };
}

/** Opens `address` to every agent. */
export function openToAll(access: MailAccess, address: string): MailAccess {
  const next = { ...access };
  delete next[key(address)];
  return next;
}

/** The Monos an account is kept for; undefined when it is open to all. */
export function readersOf(
  access: MailAccess,
  address: string,
): string[] | undefined {
  return access[key(address)];
}

export function parseMailAccess(value: unknown): MailAccess {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const access: MailAccess = {};
  for (const [address, readers] of Object.entries(value)) {
    if (!Array.isArray(readers)) continue;
    const ids = readers.filter(
      (id): id is string => typeof id === "string" && id.length > 0,
    );
    if (ids.length) access[key(address)] = ids;
  }
  return access;
}

export function loadMailAccess(): MailAccess {
  try {
    return parseMailAccess(JSON.parse(localStorage.getItem(KEY) ?? "{}"));
  } catch {
    return {};
  }
}

export function saveMailAccess(access: MailAccess): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(access));
  } catch {
    // private mode / quota
  }
}

// ---- A bot asking the user to connect an account ----

export type MailConnectRequest = {
  address: string;
  provider: "gmail" | "icloud" | "imap";
  name?: string;
  /** Who asked, as the user reads it: «Finance». */
  askedBy: string;
  /** Kept for the bot that asked, not every agent. */
  onlyFor?: string;
};

type Listener = (request: MailConnectRequest) => void;
const listeners = new Set<Listener>();

/** Opens the connect dialog where it is mounted; false when nothing shows it. */
export function requestMailConnect(request: MailConnectRequest): boolean {
  for (const listener of listeners) listener(request);
  return listeners.size > 0;
}

export function onMailConnectRequest(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The provider an address most likely uses, for the dialog to start on. */
export function providerFor(address: string): MailConnectRequest["provider"] {
  const domain = key(address).split("@")[1] ?? "";
  if (domain === "gmail.com" || domain === "googlemail.com") return "gmail";
  if (["icloud.com", "me.com", "mac.com"].includes(domain)) return "icloud";
  return "imap";
}
