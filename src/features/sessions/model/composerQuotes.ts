/**
 * Text added to the chat with "Add to chat" (from an answer, a paper, a file)
 * waits in the composer as a compact chip instead of a block of `>` lines in
 * the draft. Sending puts each one back as a quote, before what was typed, so
 * the agent reads the same message it did when quotes went into the draft.
 */
import { appendSelectionQuote } from "./quoteDraft";

export type ComposerQuote = { id: string; text: string };

/**
 * The quoted words, and the line that says where they come from when the
 * quote ends with one (`— Author (year), “Title”, p. 3 (link)`).
 */
export function splitQuoteSource(text: string): {
  body: string;
  source?: string;
} {
  const value = text.replace(/\r\n?/g, "\n").trim();
  const match = /\n\s*\n(— [^\n]+)$/.exec(value);
  if (!match) return { body: value };
  return { body: value.slice(0, match.index).trim(), source: match[1] };
}

/** The chip's words: the start of the quote on one line. */
export function quoteChipLabel(text: string, max = 64): string {
  const line = splitQuoteSource(text).body.replace(/\s+/g, " ").trim();
  return line.length > max ? `${line.slice(0, max - 1).trimEnd()}…` : line;
}

/** Where the quote is from, without the link, for under the chip's words. */
export function quoteChipSource(text: string): string | undefined {
  const source = splitQuoteSource(text).source;
  return source?.replace(/^—\s*/, "").replace(/\s*\([^()]*\)$/, "");
}

/** Adds a quote unless the same words are already waiting. */
export function addComposerQuote(
  quotes: readonly ComposerQuote[],
  text: string,
  id: string,
): ComposerQuote[] {
  const value = text.replace(/\r\n?/g, "\n").trim();
  if (!value || quotes.some((quote) => quote.text === value)) return [...quotes];
  return [...quotes, { id, text: value }];
}

/** The message as sent: every quote as a `>` block, then what was typed. */
export function withComposerQuotes(
  message: string,
  quotes: readonly ComposerQuote[],
): string {
  if (!quotes.length) return message;
  const quoted = quotes
    .reduce((draft, quote) => appendSelectionQuote(draft, quote.text), "")
    .trimEnd();
  return message.trim() ? `${quoted}\n\n${message}` : quoted;
}
