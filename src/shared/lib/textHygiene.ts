/**
 * Text as a person would have typed it: no characters nobody can see, and
 * plain spaces and line breaks. What a model writes, or what comes in from a
 * web page or a PDF, can carry zero-width marks, hidden "tag" characters,
 * direction overrides and odd spaces. They break search, word counts, diffs
 * and pasted cells, and they can hide text a reader never sees.
 *
 * Emoji keep the joiners and selectors they are made of, and code fences
 * keep their spacing.
 */

/** Invisible format marks with no place in prose. */
const INVISIBLE =
  /[\u00AD\u034F\u061C\u115F\u1160\u17B4\u17B5\u180B-\u180F\u200B\u200E\u200F\u202A-\u202E\u2060-\u2064\u2066-\u206F\u3164\uFEFF\uFFA0\uFFF9-\uFFFB]|[\u{1D173}-\u{1D17A}]|[\u{E0100}-\u{E01EF}]/gu;

/** A joiner outside an emoji sequence joins nothing. */
const LONE_ZWJ = /(?<!\p{Extended_Pictographic}\uFE0F?)\u200D|\u200D(?!\p{Extended_Pictographic})/gu;

/** A non-joiner only matters between letters of the scripts that use it. */
const LONE_ZWNJ = /(?<![\p{L}\p{M}])\u200C|\u200C(?![\p{L}\p{M}])|(?<=[\p{Script=Latin}\p{N}])\u200C|\u200C(?=[\p{Script=Latin}\p{N}])/gu;

/** Tag characters spell flags after a black flag; anywhere else they hide text. */
const TAGS = /(\u{1F3F4}[\u{E0020}-\u{E007E}]+\u{E007F})|[\u{E0000}-\u{E007F}]/gu;

/** Spaces of other widths, read as a plain space. */
const ODD_SPACES = /[\u00A0\u1680\u2000-\u200A\u202F\u205F]/g;

/** Unicode line and paragraph separators, read as a line break. */
const LINE_SEPARATORS = /[\u2028\u2029\u0085]/g;

/** Removes the invisible marks only; spacing is left as it is. */
export function stripInvisible(text: string): string {
  return text
    .replace(TAGS, (_match, flag: string | undefined) => flag ?? "")
    .replace(INVISIBLE, "")
    .replace(LONE_ZWJ, "")
    .replace(LONE_ZWNJ, "");
}

/** Whether a text carries anything `cleanText` would change. */
export function hasHiddenMarks(text: string): boolean {
  return cleanText(text) !== text;
}

/**
 * The text with its invisible marks removed, odd spaces made plain, no
 * trailing spaces on a line and no more than one blank line in a row. Inside
 * a fenced code block only the invisible marks go.
 */
export function cleanText(text: string): string {
  const lines = stripInvisible(text)
    .replace(/\r\n?/g, "\n")
    .replace(LINE_SEPARATORS, "\n")
    .split("\n");
  const out: string[] = [];
  let fence: string | null = null;
  let blank = 0;
  for (const raw of lines) {
    const marker = /^\s{0,3}(`{3,}|~{3,})/.exec(raw)?.[1];
    if (fence) {
      out.push(raw);
      if (marker && marker[0] === fence[0] && marker.length >= fence.length)
        fence = null;
      continue;
    }
    if (marker) {
      fence = marker;
      blank = 0;
      out.push(raw.trimEnd());
      continue;
    }
    const line = raw.replace(ODD_SPACES, " ").trimEnd();
    if (!line) {
      blank += 1;
      if (blank > 1) continue;
    } else blank = 0;
    out.push(line);
  }
  return out.join("\n").replace(/^\n+/, "").replace(/\n+$/, "");
}
