import { DEFAULT_AUTHSERV_ID } from "./config.ts";

const MAX_HEADER_BYTES = 64 * 1024;

/**
 * Returns the raw header block as a latin1 string, or null if no blank line
 * ends it within MAX_HEADER_BYTES (oversized or malformed header section).
 */
export function extractHeaderBlock(raw: Uint8Array): string | null {
  const limit = Math.min(raw.length, MAX_HEADER_BYTES);
  for (let i = 0; i < limit; i++) {
    if (raw[i] !== 0x0a) continue;
    // "\n\n" or "\n\r\n"; the first line may be empty only for header-less mail.
    const next = raw[i + 1];
    if (next === 0x0a || (next === 0x0d && raw[i + 2] === 0x0a)) {
      return new TextDecoder("latin1").decode(raw.subarray(0, i + 1));
    }
  }
  return raw.length <= MAX_HEADER_BYTES ? new TextDecoder("latin1").decode(raw) : null;
}

export function headerValues(block: string, name: string): string[] {
  const wanted = name.toLowerCase();
  const unfolded = block.replace(/\r?\n[ \t]+/g, " ");
  const values: string[] = [];
  for (const line of unfolded.split(/\r?\n/)) {
    const colon = line.indexOf(":");
    if (colon <= 0) continue;
    if (line.slice(0, colon).trim().toLowerCase() === wanted) {
      values.push(line.slice(colon + 1).trim());
    }
  }
  return values;
}

function stripComments(value: string): string {
  let previous: string;
  let current = value;
  do {
    previous = current;
    current = current.replace(/\([^()]*\)/g, " ");
  } while (current !== previous);
  return current;
}

interface MethodResult {
  result: string;
  props: Map<string, string>;
}

interface ParsedAuthResults {
  authservId: string;
  methods: Map<string, MethodResult[]>;
}

function parseAuthResults(value: string): ParsedAuthResults | null {
  const parts = stripComments(value).split(";");
  const authservId = (parts[0] ?? "").trim().split(/\s+/)[0]?.toLowerCase();
  if (!authservId) return null;
  const methods = new Map<string, MethodResult[]>();
  for (const part of parts.slice(1)) {
    const head = /^\s*([a-z0-9-]+)\s*=\s*([a-z]+)\b(.*)$/is.exec(part);
    if (!head) continue;
    const method = (head[1] ?? "").toLowerCase();
    const props = new Map<string, string>();
    for (const prop of (head[3] ?? "").matchAll(/([a-z]+\.[a-z0-9_-]+)\s*=\s*("[^"]*"|[^\s;]+)/gi)) {
      props.set((prop[1] ?? "").toLowerCase(), (prop[2] ?? "").replace(/^"|"$/g, "").toLowerCase());
    }
    const list = methods.get(method) ?? [];
    list.push({ result: (head[2] ?? "").toLowerCase(), props });
    methods.set(method, list);
  }
  return { authservId, methods };
}

function domainOf(value: string): string {
  return value.slice(value.lastIndexOf("@") + 1).replace(/^<|>$/g, "");
}

/**
 * True unless the message carries exactly one Authentication-Results header
 * from the trusted authserv-id proving SPF on the envelope sender's domain and
 * DMARC.
 *
 * Senders control every header in the raw message, so a forged header cannot be
 * told apart from the real one by content. The defence is structural:
 * Cloudflare prepends its own header, therefore a forgery shows up as a second
 * header with the trusted id and is rejected as ambiguous. This relies on
 * Cloudflare always adding its header; verify that on your zone (README).
 */
export function isAuthFailed(
  raw: Uint8Array,
  envelopeFrom: string,
  trustedAuthservId: string = DEFAULT_AUTHSERV_ID,
): boolean {
  const block = extractHeaderBlock(raw);
  if (block === null) return true;
  const trusted = trustedAuthservId.trim().toLowerCase();
  const candidates = headerValues(block, "Authentication-Results")
    .map(parseAuthResults)
    .filter((parsed): parsed is ParsedAuthResults => parsed?.authservId === trusted);
  const only = candidates[0];
  if (candidates.length !== 1 || only === undefined) return true;

  const spf = only.methods.get("spf");
  const dmarc = only.methods.get("dmarc");
  if (spf?.length !== 1 || dmarc?.length !== 1) return true;
  const spfResult = spf[0];
  if (spfResult?.result !== "pass" || dmarc[0]?.result !== "pass") return true;

  const mailFrom = spfResult.props.get("smtp.mailfrom");
  if (mailFrom === undefined) return true;
  return domainOf(mailFrom) !== domainOf(envelopeFrom.toLowerCase());
}
