import type { Env } from "./types.ts";

export const MAX_MESSAGE_BYTES = 2 * 1024 * 1024;
export const MIN_TOKEN_LENGTH = 32;
export const DEFAULT_RETENTION_DAYS = 30;
export const MAX_RETENTION_DAYS = 365;
export const DEFAULT_AUTHSERV_ID = "mx.cloudflare.net";
export const R2_KEY_PREFIX = "mail/";

export interface RecipientAllowlist {
  addresses: ReadonlySet<string>;
  domains: ReadonlySet<string>;
}

export function parseAllowlist(raw: string | undefined): RecipientAllowlist | null {
  const addresses = new Set<string>();
  const domains = new Set<string>();
  for (const entry of (raw ?? "").split(",")) {
    const value = entry.trim().toLowerCase();
    if (value === "") continue;
    if (value.startsWith("*@")) {
      const domain = value.slice(2);
      if (!isDomain(domain)) return null;
      domains.add(domain);
    } else {
      if (!isAddress(value)) return null;
      addresses.add(value);
    }
  }
  if (addresses.size === 0 && domains.size === 0) return null;
  return { addresses, domains };
}

export function isAllowedRecipient(list: RecipientAllowlist, address: string): boolean {
  if (list.addresses.has(address)) return true;
  const at = address.lastIndexOf("@");
  return at > 0 && list.domains.has(address.slice(at + 1));
}

const DOMAIN_RE = /^(?=.{1,253}$)([a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/;
const ADDRESS_RE = /^[a-z0-9!#$%&'*+/=?^_`{|}~.-]{1,64}@([^@\s]+)$/;

function isDomain(value: string): boolean {
  return DOMAIN_RE.test(value);
}

function isAddress(value: string): boolean {
  const match = ADDRESS_RE.exec(value);
  return match !== null && isDomain(match[1] ?? "");
}

/** Lowercased envelope address, or null when it is not a plain mailbox. */
export function normalizeEnvelopeAddress(value: string): string | null {
  const address = value.trim().toLowerCase();
  return address.length <= 254 && isAddress(address) ? address : null;
}

export function hasValidToken(env: Env): env is Env & { INBOUND_TOKEN: string } {
  return typeof env.INBOUND_TOKEN === "string" && env.INBOUND_TOKEN.length >= MIN_TOKEN_LENGTH;
}

export function retentionDays(raw: string | undefined): number {
  if (raw === undefined || !/^\d{1,3}$/.test(raw.trim())) return DEFAULT_RETENTION_DAYS;
  const days = Number(raw.trim());
  return days >= 1 && days <= MAX_RETENTION_DAYS ? days : DEFAULT_RETENTION_DAYS;
}
