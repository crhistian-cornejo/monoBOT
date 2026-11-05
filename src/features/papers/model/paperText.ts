/**
 * A paper's text, page by page, and how an agent reads it without the whole
 * PDF: the abstract and the opening pages for a summary, chosen pages when it
 * asks for them, or the passages that best match a question.
 */

export type PaperText = {
  /** Text of each page, page 1 first. */
  pages: string[];
};

const STOP = new Set(
  "the a an of and or to in on for with by from is are was were be this that these those it its as at not we our can has have had which also than then into over under between using use used".split(
    " ",
  ),
);

export function terms(query: string): string[] {
  return [
    ...new Set(
      query
        .toLowerCase()
        .split(/[^\p{L}\p{N}]+/u)
        .filter((term) => term.length > 2 && !STOP.has(term)),
    ),
  ];
}

export type Passage = { page: number; text: string; score: number };

const BIBLIOGRAPHY =
  /^\s*(references|bibliography|literature cited|works cited)\s*$/im;

/** The pages up to where the bibliography starts: its entries answer nothing. */
function body(text: PaperText): string[] {
  const kept: string[] = [];
  for (const page of text.pages) {
    const start = page.search(BIBLIOGRAPHY);
    if (start < 0) {
      kept.push(page);
      continue;
    }
    kept.push(page.slice(0, start));
    break;
  }
  return kept;
}

/** Paragraph-sized pieces of every page, in reading order. */
export function passages(text: PaperText): Passage[] {
  return body(text).flatMap((page, index) =>
    page
      .split(/\n{2,}|(?<=[.!?])\s{2,}/)
      .map((piece) => piece.replace(/\s+/g, " ").trim())
      .filter((piece) => piece.length >= 60)
      .map((piece) => ({ page: index + 1, text: piece, score: 0 })),
  );
}

/** A word's stem, crudely: "calibrated" finds "calibration". */
const stem = (term: string) =>
  term.length > 6 ? term.slice(0, Math.max(5, term.length - 3)) : term;

/** The passages that share the most terms with the query. */
export function bestPassages(
  text: PaperText,
  query: string,
  limit = 6,
): Passage[] {
  const wanted = terms(query);
  if (!wanted.length) return [];
  return (
    passages(text)
      .map((passage) => {
        const lower = passage.text.toLowerCase();
        const hits = wanted.filter((term) => lower.includes(stem(term))).length;
        return { ...passage, score: hits / Math.sqrt(passage.text.length) };
      })
      .filter((passage) => passage.score > 0)
      .sort((a, b) => b.score - a.score)
      .slice(0, limit)
      // The best few, read in the order the paper tells them.
      .sort((a, b) => a.page - b.page)
  );
}

/** "3", "2-4" or "1,3-5" as page numbers within 1..total. */
export function parsePages(spec: string, total: number): number[] {
  const wanted = new Set<number>();
  for (const part of spec.split(",")) {
    const range = part.trim().match(/^(\d+)(?:\s*-\s*(\d+))?$/);
    if (!range) continue;
    const from = Number(range[1]);
    const to = Number(range[2] ?? range[1]);
    for (let page = from; page <= Math.min(to, total); page += 1)
      if (page >= 1) wanted.add(page);
  }
  return [...wanted].sort((a, b) => a - b);
}

export type Excerpt = {
  pageCount: number;
  /** What came back: pages "1-3", passages for a question, or the opening. */
  covers: string;
  text: string;
  truncated: boolean;
};

/**
 * What the agent receives from one read. `maxChars` keeps a long paper from
 * flooding its context: the rest is one more call away.
 */
export function excerpt(
  text: PaperText,
  options: { pages?: string; query?: string; maxChars?: number },
): Excerpt {
  const maxChars = options.maxChars ?? 24_000;
  const pageCount = text.pages.length;
  let body: string;
  let covers: string;
  if (options.pages) {
    const pages = parsePages(options.pages, pageCount);
    body = pages
      .map((page) => `[page ${page}]\n${text.pages[page - 1].trim()}`)
      .join("\n\n");
    covers = `pages ${options.pages}`;
  } else if (options.query) {
    const found = bestPassages(text, options.query, 8);
    body = found
      .map((passage) => `[page ${passage.page}] ${passage.text}`)
      .join("\n\n");
    covers = found.length
      ? `${found.length} passages for "${options.query}"`
      : `no passage matches "${options.query}"`;
  } else {
    body = text.pages
      .map((page, index) => `[page ${index + 1}]\n${page.trim()}`)
      .join("\n\n");
    covers = "the paper from the start";
  }
  const truncated = body.length > maxChars;
  return {
    pageCount,
    covers: truncated ? `${covers}, cut at ${maxChars} characters` : covers,
    text: truncated ? body.slice(0, maxChars) : body,
    truncated,
  };
}

export type OutlineEntry = {
  title: string;
  /** 1 for a chapter ("3 Model"), 2 for "3.1 Encoder", and so on. */
  level: number;
  page: number;
  /** Position among the paper's lines, to cut a section out. */
  line: number;
};

type Line = { page: number; text: string };

function lines(text: PaperText): Line[] {
  return text.pages.flatMap((page, index) =>
    page.split("\n").map((raw) => ({ page: index + 1, text: raw.trim() })),
  );
}

const PLAIN_HEADING =
  /^(abstract|introduction|background|related work|literature review|methods?|methodology|materials and methods|experimental setup|experiments?|results|results and discussion|evaluation|discussion|conclusions?|conclusions and future work|acknowledge?ments|limitations|references|bibliography|appendix(?: [a-z0-9])?)\.?$/i;
const NUMBERED_HEADING =
  /^(\d{1,2}(?:\.\d{1,2}){0,2})\.?\s+([A-Z][^.\n]{2,80})$/;

/**
 * The paper's section headings, found in its text: numbered ones ("3.1 Encoder
 * and Decoder Stacks") and the usual unnumbered ones ("Methods"). They are how
 * an agent asks for the methodology without reading the whole paper.
 */
export function outline(text: PaperText): OutlineEntry[] {
  const entries: OutlineEntry[] = [];
  const seen = new Set<string>();
  lines(text).forEach((line, index) => {
    const value = line.text;
    if (value.length < 3 || value.length > 90) return;
    if (/^(figure|fig\.|table)\s/i.test(value)) return;
    let title: string | undefined;
    let level = 1;
    const numbered = NUMBERED_HEADING.exec(value);
    if (numbered && !/^\d+$/.test(numbered[2])) {
      level = numbered[1].split(".").length;
      title = `${numbered[1]} ${numbered[2].trim()}`;
    } else if (PLAIN_HEADING.test(value)) title = value.replace(/\.$/, "");
    if (!title) return;
    const key = title.toLowerCase();
    if (seen.has(key)) return;
    seen.add(key);
    entries.push({ title, level, page: line.page, line: index });
  });
  return entries.slice(0, 80);
}

export type Figure = { label: string; page: number; caption: string };

const CAPTION = /^(Figure|Fig\.|Table)\s+(\d{1,3})\s*[.:]\s*(\S.{8,240})$/;

/** Figures and tables, by their captions: "Figure 2" is on page 4. */
export function figures(text: PaperText): Figure[] {
  const found = new Map<string, Figure>();
  for (const line of lines(text)) {
    const match = CAPTION.exec(line.text);
    if (!match) continue;
    const label = `${match[1].startsWith("Fig") ? "Figure" : "Table"} ${match[2]}`;
    if (!found.has(label))
      found.set(label, { label, page: line.page, caption: match[3].trim() });
  }
  return [...found.values()];
}

const SYNONYMS: [RegExp, RegExp][] = [
  [
    /method|approach/i,
    /method|approach|materials|model|experimental setup|setup/i,
  ],
  [/result|finding/i, /result|experiment|evaluation|finding/i],
  [/conclu|summary/i, /conclu|summary|discussion/i],
  [/intro/i, /introduction|background/i],
  [/related|prior/i, /related|background|literature/i],
];

/** The words of a section, from its heading to the next of its level or above. */
export function section(
  text: PaperText,
  name: string,
  maxChars = 24_000,
):
  | { title: string; page: number; text: string; truncated: boolean }
  | undefined {
  const all = lines(text);
  const headings = outline(text);
  const wanted = name.trim().toLowerCase();
  const alike = SYNONYMS.find(([asked]) => asked.test(wanted))?.[1];
  const start =
    headings.find((entry) => entry.title.toLowerCase().includes(wanted)) ??
    (alike ? headings.find((entry) => alike.test(entry.title)) : undefined);
  if (!start) return undefined;
  const next = headings.find(
    (entry) => entry.line > start.line && entry.level <= start.level,
  );
  const body = all
    .slice(start.line, next ? next.line : all.length)
    .map((line) => line.text)
    .join("\n")
    .trim();
  const truncated = body.length > maxChars;
  return {
    title: start.title,
    page: start.page,
    text: truncated ? body.slice(0, maxChars) : body,
    truncated,
  };
}

/**
 * The paper's own DOI, from its first pages (journals print it in the header):
 * how a PDF the user gives is matched with the paper it is.
 */
export function detectDoi(text: PaperText): string | undefined {
  const head = text.pages.slice(0, 3).join("\n");
  const match = head.match(/\b10\.\d{4,9}\/[^\s"<>)\]]+/);
  return match?.[0].replace(/[.,;:]+$/, "").toLowerCase();
}

const NOT_A_TITLE =
  /https?:|www\.|doi|@|©|copyright|contents lists|issn|received|accepted|available online|journal|volume|vol\.|preprint|arxiv|^\d+$/i;

/**
 * A guess at the paper's title from its first page: the longest line of the
 * opening that is not a journal header or a notice. Only a guess; the agent
 * can pass the real title.
 */
export function guessTitle(text: PaperText): string | undefined {
  const lines = (text.pages[0] ?? "")
    .split("\n")
    .map((line) => line.trim())
    .slice(0, 14);
  // The title comes before the abstract, never in it.
  const abstract = lines.findIndex((line) => /^abstract\b/i.test(line));
  const opening = (abstract < 0 ? lines : lines.slice(0, abstract)).filter(
    (line) =>
      line.length >= 15 && line.length <= 200 && !NOT_A_TITLE.test(line),
  );
  return opening.sort((a, b) => b.length - a.length)[0];
}

/** The abstract as the paper prints it, from "Abstract" to the keywords or the introduction. */
export function guessAbstract(text: PaperText): string | undefined {
  const head = text.pages
    .slice(0, 2)
    .join("\n")
    .split("\n")
    .map((line) => line.trim());
  const start = head.findIndex((line) => /^abstract\b/i.test(line));
  if (start < 0) return undefined;
  const body: string[] = [head[start].replace(/^abstract\b[\s:.\-—]*/i, "")];
  for (const line of head.slice(start + 1)) {
    if (/^(keywords?|key words|1\.?\s+introduction|introduction)\b/i.test(line))
      break;
    body.push(line);
    if (body.join(" ").length > 2500) break;
  }
  const abstract = body.join(" ").replace(/\s+/g, " ").trim();
  return abstract.length >= 80 ? abstract : undefined;
}
