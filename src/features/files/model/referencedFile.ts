import { resolveWorkspacePath } from "../../../shared/lib/paths";

/**
 * Where a file an agent named may be. An agent often names a file by itself
 * (`LOB.xlsx`) after writing it with its full path, and in a group the member
 * that wrote it works in a folder of its own, so the conversation's folder
 * alone does not find it.
 */

/** Past this many candidates the rest are not checked; the disk is asked once. */
export const MAX_CANDIDATES = 64;

/** Every string in a value, depth first, up to a total length. */
export function collectTexts(value: unknown, budget = 4_000_000): string[] {
  const out: string[] = [];
  let left = budget;
  const visit = (entry: unknown, depth: number) => {
    if (left <= 0 || depth > 12) return;
    if (typeof entry === "string") {
      if (entry.length > 3) {
        out.push(entry);
        left -= entry.length;
      }
    } else if (Array.isArray(entry))
      for (const item of entry) visit(item, depth + 1);
    else if (entry && typeof entry === "object")
      for (const item of Object.values(entry)) visit(item, depth + 1);
  };
  visit(value, 0);
  return out;
}

function fileNameOf(reference: string): string {
  return reference.split(/[\\/]/).filter(Boolean).pop() ?? reference;
}

const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Absolute paths in the texts that end with the file's name, most recent
 * mention first. A path may hold spaces, so it runs back from the name to a
 * drive letter or root, never across a line or a Markdown delimiter.
 */
export function mentionedPaths(
  texts: readonly string[],
  reference: string,
): string[] {
  const name = fileNameOf(reference);
  if (!name) return [];
  // A drive (`C:\`, `P:/`), a share (`\\server`) or a root (`/home`), then
  // anything but a line break or a Markdown delimiter, up to the name.
  const pattern = new RegExp(
    String.raw`(?:[A-Za-z]:[\\/]|\\\\|/)[^\n\r<>"'` +
      "`" +
      String.raw`|*?\[\]]*?[\\/]${escape(name)}(?![\w.-])`,
    "gi",
  );
  const found: string[] = [];
  const add = (path: string) => {
    if (!found.includes(path)) found.push(path);
  };
  for (let index = texts.length - 1; index >= 0; index--) {
    const text = texts[index];
    if (!text.toLowerCase().includes(name.toLowerCase())) continue;
    const matches = [...text.matchAll(pattern)].map((match) => match[0]);
    for (const match of matches.reverse()) {
      // A match may start too early, at a slash in the words before the
      // path: each later root in it is a candidate too, and the disk decides.
      for (let at = 0; at < match.length; at++) {
        const rest = match.slice(at);
        const root =
          /^[A-Za-z]:[\\/]/.test(rest) ||
          ((rest.startsWith("/") || rest.startsWith("\\\\")) &&
            (at === 0 || /\s/.test(match[at - 1])));
        if (!root) continue;
        // `/C:/x`, as a Markdown link or a file URL leaves it, is `C:/x`.
        add(rest.replace(/^\/(?=[A-Za-z]:[\\/])/, ""));
      }
    }
  }
  return found;
}

/**
 * The paths to check, best first: the reference itself when absolute, then
 * against each folder an agent of the conversation works in, then where the
 * conversation said it wrote a file by that name.
 */
export function candidatePaths(
  reference: string,
  folders: readonly (string | undefined)[],
  mentioned: readonly string[],
): string[] {
  const out: string[] = [];
  const add = (path: string | undefined) => {
    if (!path) return;
    const key = path.replace(/\\/g, "/").toLowerCase();
    if (!out.some((entry) => entry.replace(/\\/g, "/").toLowerCase() === key))
      out.push(path);
  };
  for (const folder of folders) add(resolveWorkspacePath(reference, folder));
  for (const path of mentioned) add(path);
  return out.slice(0, MAX_CANDIDATES);
}

/**
 * The first candidate that is a file on disk, or undefined. `stat` answers
 * for many paths at once, as the app's `statFiles` does.
 */
export async function locateReferencedFile(
  reference: string,
  folders: readonly (string | undefined)[],
  texts: readonly string[],
  stat: (
    paths: string[],
  ) => Promise<{ path: string; mtimeMs: number | null }[]>,
): Promise<string | undefined> {
  const candidates = candidatePaths(
    reference,
    folders,
    mentionedPaths(texts, reference),
  );
  if (!candidates.length) return undefined;
  const found = await stat(candidates).catch(() => []);
  return candidates.find(
    (path) => found.find((entry) => entry.path === path)?.mtimeMs != null,
  );
}
