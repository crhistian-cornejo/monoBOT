/**
 * The faces beside the people of a reply: the photo the user chose for their
 * own account, else Gravatar's for the address, else their initial (the UI's
 * fallback). Pure but for the browser's storage and hashing; no React.
 */

const PHOTO_KEY = "monocode.mail.photo.";

/** The largest side a chosen photo is kept at, in pixels. */
export const PHOTO_SIZE = 128;

/** Fired on `window` when the user chooses or removes a photo, so faces already on screen change. */
export const PHOTO_EVENT = "monocode-mail-photo";

const key = (address: string) => PHOTO_KEY + address.trim().toLowerCase();

/** The photo the user chose for one of their accounts, as a data URL. */
export function loadMyPhoto(address: string): string | undefined {
  try {
    return localStorage.getItem(key(address)) ?? undefined;
  } catch {
    return undefined;
  }
}

/** Keeps the photo for an account, or forgets it with `undefined`. */
export function saveMyPhoto(address: string, photo: string | undefined): void {
  try {
    if (photo) localStorage.setItem(key(address), photo);
    else localStorage.removeItem(key(address));
  } catch {
    // private mode / quota
  }
  globalThis.dispatchEvent?.(new Event(PHOTO_EVENT));
}

/**
 * Gravatar's picture for an address, by its SHA-256 (the address itself never
 * leaves): a 404 when there is none, so the initial shows instead.
 */
export async function gravatarUrl(address: string, size = 64): Promise<string | undefined> {
  const email = address.trim().toLowerCase();
  if (!email.includes("@") || !globalThis.crypto?.subtle) return undefined;
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(email));
  const hex = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `https://gravatar.com/avatar/${hex}?s=${size}&d=404`;
}

/** Mail providers whose icon says nothing about who someone is. */
const PERSONAL_DOMAINS = new Set([
  "gmail.com",
  "googlemail.com",
  "icloud.com",
  "me.com",
  "mac.com",
  "outlook.com",
  "hotmail.com",
  "live.com",
  "msn.com",
  "yahoo.com",
  "ymail.com",
  "aol.com",
  "proton.me",
  "protonmail.com",
  "gmx.com",
  "zoho.com",
]);

/**
 * The organisation an address belongs to, whose icon shows beside it: its
 * domain, or nothing for a personal mail provider.
 */
export function organisationDomain(address: string): string | undefined {
  const at = address.lastIndexOf("@");
  if (at < 0) return undefined;
  const domain = address.slice(at + 1).trim().toLowerCase();
  if (!domain.includes(".") || PERSONAL_DOMAINS.has(domain)) return undefined;
  if (/^(?:[a-z]+\.)?(?:gmail|hotmail|outlook|yahoo|live)\.[a-z.]+$/.test(domain)) return undefined;
  return domain;
}
