/**
 * Where on the web each PDF without a DOI came from. A report or a guide the
 * agent downloaded is kept under a fingerprint of its bytes (`pdf:…`), but its
 * answer cites the link it was found at; this maps that link back to the PDF,
 * so the citation opens and underlines it like a paper's.
 */

const STORE = "monocode.paperSources";
const LIMIT = 400;

/** The same address however it was written: no fragment, lower-case host. */
export function normalizeSourceUrl(value: string): string | undefined {
  let url: URL;
  try {
    url = new URL(value.trim());
  } catch {
    return undefined;
  }
  if (url.protocol !== "http:" && url.protocol !== "https:") return undefined;
  url.hash = "";
  const path = url.pathname.replace(/\/+$/, "") || "/";
  return `${url.host.toLowerCase()}${path}${url.search}`;
}

function read(): Record<string, string> {
  try {
    const raw = localStorage.getItem(STORE);
    const parsed = raw ? (JSON.parse(raw) as unknown) : {};
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, string>)
      : {};
  } catch {
    return {};
  }
}

/** Records that `key` is the PDF found at each of these links. */
export function rememberPaperSource(
  links: readonly (string | undefined)[],
  key: string,
): void {
  const entries = read();
  let changed = false;
  for (const link of links) {
    const at = link ? normalizeSourceUrl(link) : undefined;
    if (!at) continue;
    // Newest last, so the oldest go first past the limit.
    delete entries[at];
    entries[at] = key;
    changed = true;
  }
  if (!changed) return;
  const kept = Object.entries(entries).slice(-LIMIT);
  try {
    localStorage.setItem(STORE, JSON.stringify(Object.fromEntries(kept)));
  } catch {
    // Without storage the citation opens the link in the browser, as before.
  }
}

/** The PDF a link was downloaded from, when the app kept it. */
export function paperKeyOfSource(link: string): string | undefined {
  const at = normalizeSourceUrl(link);
  if (!at) return undefined;
  const key = read()[at];
  return typeof key === "string" && key.startsWith("pdf:") ? key : undefined;
}
