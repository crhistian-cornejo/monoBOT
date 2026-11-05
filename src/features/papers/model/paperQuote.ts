/**
 * Text selected in a paper, made ready for the chat or a note. pdf.js lays a
 * page's text out line by line, so a selection carries the paper's line
 * breaks and the hyphens that split words across them; both are undone here.
 * The quote then names where it came from, so whoever reads it (the user, or
 * the agent it is sent to) can go back to the paper and the page.
 */

/** One line of prose from a selection in the PDF's text layer. */
export function cleanPdfSelection(text: string): string {
  return (
    text
      .replace(/\r\n?/g, "\n")
      // "increas-\ning" is one word broken by the layout, not a hyphenated one.
      .replace(/(\p{L})-\n\s*(\p{Ll})/gu, "$1$2")
      .replace(/\s+/g, " ")
      .trim()
  );
}

export type PaperQuoteSource = {
  /** First author and year: "Chlumsky et al. (2022)". */
  label?: string;
  title: string;
  page?: number;
  /** The paper's own link (DOI, arXiv or where its PDF was found). */
  link?: string;
};

/** Where a quote comes from, as one line: label, title, page and link. */
export function paperSourceLine(source: PaperQuoteSource): string {
  const parts = [source.label?.trim(), `“${source.title.trim()}”`].filter(
    Boolean,
  );
  if (source.page) parts.push(`p. ${source.page}`);
  const line = `— ${parts.join(", ")}`;
  return source.link ? `${line} (${source.link})` : line;
}

/** The selected words, then the line that says where they are from. */
export function paperQuote(text: string, source: PaperQuoteSource): string {
  const quoted = cleanPdfSelection(text);
  return quoted ? `${quoted}\n\n${paperSourceLine(source)}` : "";
}
