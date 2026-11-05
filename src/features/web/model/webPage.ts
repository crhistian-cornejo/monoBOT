/**
 * A web page as an agent should read it: its main text as light Markdown,
 * who published it and when, its picture and icon, the PDF it offers, and the
 * links worth following. The HTML comes from the app's guarded fetch
 * (src-tauri/src/web.rs); DOMParser never runs the page's scripts.
 */

export type WebLink = { text: string; url: string };

export type WebPageRead = {
  url: string;
  title?: string;
  description?: string;
  siteName: string;
  author?: string;
  published?: string;
  modified?: string;
  language?: string;
  /** og:image, for a preview card. */
  image?: string;
  /** The site's icon: the page's own, else /favicon.ico. */
  icon: string;
  canonical?: string;
  /** A scholarly page's DOI and PDF (citation_* tags): read them as a paper. */
  doi?: string;
  pdfUrl?: string;
  outline: string[];
  text: string;
  /** Characters of main text before it was cut to `maxChars`. */
  chars: number;
  truncated: boolean;
  /** The paragraphs that best match `query`, when one was asked. */
  passages?: string[];
  links: WebLink[];
};

const REMOVED = [
  "script",
  "style",
  "noscript",
  "template",
  "svg",
  "canvas",
  "iframe",
  "object",
  "nav",
  "footer",
  "aside",
  "form",
  "button",
  "dialog",
  "[role=navigation]",
  "[role=banner]",
  "[role=contentinfo]",
  "[role=complementary]",
  "[aria-hidden=true]",
  "[hidden]",
].join(",");

const BLOCKS = new Set([
  "address",
  "article",
  "blockquote",
  "dd",
  "details",
  "div",
  "dl",
  "dt",
  "figcaption",
  "figure",
  "header",
  "hr",
  "li",
  "main",
  "ol",
  "p",
  "section",
  "summary",
  "ul",
]);

const MAX_LINKS = 40;
const MAX_OUTLINE = 30;

export function readWebPage(
  html: string,
  url: string,
  options: { maxChars?: number; query?: string } = {},
): WebPageRead {
  const doc = new DOMParser().parseFromString(html, "text/html");
  const base = safeUrl(url);
  const meta = metaReader(doc);
  const linked = linkedData(doc);
  const absolute = (href: string | undefined | null) =>
    href ? resolve(href, base) : undefined;

  const title =
    meta("og:title", "twitter:title", "citation_title", "dc.title") ??
    linked.headline ??
    clean(doc.title) ??
    clean(doc.querySelector("h1")?.textContent);
  const description = meta(
    "og:description",
    "twitter:description",
    "description",
    "dc.description",
  );
  const siteName =
    meta("og:site_name", "application-name", "citation_journal_title") ??
    linked.publisher ??
    base?.hostname.replace(/^www\./, "") ??
    "";
  const authors = metaAll(doc, "citation_author", "author", "article:author");
  const author =
    (authors.length ? authors.slice(0, 6).join(", ") : undefined) ??
    linked.author;
  const published =
    meta(
      "article:published_time",
      "citation_publication_date",
      "citation_date",
      "dc.date",
      "date",
    ) ??
    linked.published ??
    doc.querySelector("time[datetime]")?.getAttribute("datetime") ??
    undefined;
  const modified = meta("article:modified_time") ?? linked.modified;
  const doi = meta("citation_doi", "dc.identifier", "prism.doi")?.match(
    /10\.\d{4,9}\/\S+/,
  )?.[0];
  const pdfUrl = absolute(meta("citation_pdf_url"));
  const image = absolute(meta("og:image", "twitter:image", "og:image:url"));
  const icon =
    absolute(linkHref(doc, "icon") ?? linkHref(doc, "apple-touch-icon")) ??
    (base ? `${base.origin}/favicon.ico` : "");
  const canonical = absolute(linkHref(doc, "canonical"));
  const language = clean(doc.documentElement.getAttribute("lang"));

  doc.querySelectorAll(REMOVED).forEach((element) => element.remove());
  const root = mainContent(doc);
  const outline = Array.from(root.querySelectorAll("h1, h2, h3"))
    .map((heading) => clean(heading.textContent))
    .filter((heading): heading is string => !!heading)
    .slice(0, MAX_OUTLINE);
  const links = pageLinks(root, base);
  const full = toMarkdown(root);
  const maxChars = options.maxChars ?? 20_000;
  const text = cut(full, maxChars);
  const passages = options.query?.trim()
    ? bestParagraphs(full, options.query)
    : undefined;

  return {
    url: base?.toString() ?? url,
    ...(title ? { title } : {}),
    ...(description ? { description: description.slice(0, 400) } : {}),
    siteName,
    ...(author ? { author } : {}),
    ...(published ? { published } : {}),
    ...(modified ? { modified } : {}),
    ...(language ? { language } : {}),
    ...(image ? { image } : {}),
    icon,
    ...(canonical && canonical !== base?.toString() ? { canonical } : {}),
    ...(doi ? { doi } : {}),
    ...(pdfUrl ? { pdfUrl } : {}),
    outline,
    text,
    chars: full.length,
    truncated: text.length < full.length,
    ...(passages ? { passages } : {}),
    links,
  };
}

function safeUrl(url: string): URL | undefined {
  try {
    return new URL(url);
  } catch {
    return undefined;
  }
}

function resolve(href: string, base: URL | undefined): string | undefined {
  try {
    const url = new URL(href.trim(), base);
    return url.protocol === "https:" || url.protocol === "http:"
      ? url.toString()
      : undefined;
  } catch {
    return undefined;
  }
}

/** The href of the first `<link>` whose rel names `kind` ("shortcut icon" too). */
function linkHref(doc: Document, kind: string): string | undefined {
  for (const link of Array.from(doc.querySelectorAll("link[rel][href]"))) {
    const rel = (link.getAttribute("rel") ?? "").toLowerCase().split(/\s+/);
    if (rel.includes(kind)) return link.getAttribute("href") ?? undefined;
  }
  return undefined;
}

function clean(value: string | null | undefined): string | undefined {
  const text = value?.replace(/\s+/g, " ").trim();
  return text ? text : undefined;
}

/** The first non-empty `<meta>` among `keys`, by property or name. */
function metaReader(doc: Document) {
  return (...keys: string[]) => {
    for (const key of keys) {
      const value = metaAll(doc, key)[0];
      if (value) return value;
    }
    return undefined;
  };
}

function metaAll(doc: Document, ...keys: string[]): string[] {
  const wanted = new Set(keys.map((key) => key.toLowerCase()));
  const values: string[] = [];
  for (const element of Array.from(doc.querySelectorAll("meta"))) {
    const key = (
      element.getAttribute("property") ??
      element.getAttribute("name") ??
      element.getAttribute("itemprop") ??
      ""
    ).toLowerCase();
    if (!wanted.has(key)) continue;
    const value = clean(element.getAttribute("content"));
    if (value && !values.includes(value)) values.push(value);
  }
  return values;
}

type LinkedData = {
  headline?: string;
  author?: string;
  publisher?: string;
  published?: string;
  modified?: string;
};

/** schema.org JSON-LD: an article's headline, authors, publisher and dates. */
function linkedData(doc: Document): LinkedData {
  const found: LinkedData = {};
  for (const script of Array.from(
    doc.querySelectorAll('script[type="application/ld+json"]'),
  )) {
    let data: unknown;
    try {
      data = JSON.parse(script.textContent ?? "");
    } catch {
      continue;
    }
    for (const item of flatten(data)) {
      found.headline ??= text(item.headline) ?? text(item.name);
      found.author ??= names(item.author);
      found.publisher ??= names(item.publisher);
      found.published ??= text(item.datePublished);
      found.modified ??= text(item.dateModified);
    }
  }
  return found;
}

function flatten(data: unknown): Record<string, unknown>[] {
  if (Array.isArray(data)) return data.flatMap(flatten);
  if (!data || typeof data !== "object") return [];
  const item = data as Record<string, unknown>;
  return [item, ...flatten(item["@graph"])];
}

function text(value: unknown): string | undefined {
  return typeof value === "string" ? clean(value) : undefined;
}

function names(value: unknown): string | undefined {
  const list = (Array.isArray(value) ? value : [value])
    .map((entry) =>
      typeof entry === "string"
        ? clean(entry)
        : entry && typeof entry === "object"
          ? text((entry as Record<string, unknown>).name)
          : undefined,
    )
    .filter((name): name is string => !!name);
  return list.length ? list.slice(0, 6).join(", ") : undefined;
}

/** The article, else main, else the body: wherever the page keeps its text. */
function mainContent(doc: Document): Element {
  const articles = Array.from(doc.querySelectorAll("article"));
  const longest = articles.sort(
    (a, b) => (b.textContent?.length ?? 0) - (a.textContent?.length ?? 0),
  )[0];
  if (longest && (longest.textContent?.trim().length ?? 0) > 200)
    return longest;
  return (
    doc.querySelector("main, [role=main]") ??
    doc.body ??
    doc.documentElement
  );
}

function pageLinks(root: Element, base: URL | undefined): WebLink[] {
  const seen = new Set<string>();
  const links: WebLink[] = [];
  for (const anchor of Array.from(root.querySelectorAll("a[href]"))) {
    const url = resolve(anchor.getAttribute("href") ?? "", base);
    if (!url) continue;
    const plain = url.split("#")[0];
    if (base && plain === base.toString().split("#")[0]) continue;
    if (seen.has(plain)) continue;
    const label = clean(anchor.textContent) ?? clean(anchor.getAttribute("title"));
    if (!label) continue;
    seen.add(plain);
    links.push({ text: label.slice(0, 80), url });
    if (links.length >= MAX_LINKS) break;
  }
  return links;
}

/** Headings, paragraphs, lists, quotes, code and tables as light Markdown. */
function toMarkdown(root: Element): string {
  const blocks: string[] = [];
  let line = "";
  const flush = () => {
    const value = line.replace(/[ \t]+/g, " ").replace(/ *\n */g, "\n").trim();
    if (value) blocks.push(value);
    line = "";
  };
  const walk = (node: Node) => {
    for (const child of Array.from(node.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        line += (child.textContent ?? "").replace(/\s+/g, " ");
        continue;
      }
      if (child.nodeType !== Node.ELEMENT_NODE) continue;
      const element = child as Element;
      const tag = element.tagName.toLowerCase();
      const heading = /^h([1-6])$/.exec(tag);
      if (heading) {
        flush();
        const value = clean(element.textContent);
        if (value) blocks.push(`${"#".repeat(Number(heading[1]))} ${value}`);
      } else if (tag === "li") {
        flush();
        const value = clean(element.textContent);
        if (value) blocks.push(`- ${value}`);
      } else if (tag === "pre") {
        flush();
        const value = element.textContent?.replace(/\s+$/, "");
        if (value?.trim()) blocks.push("```\n" + value + "\n```");
      } else if (tag === "blockquote") {
        flush();
        const value = clean(element.textContent);
        if (value) blocks.push(`> ${value}`);
      } else if (tag === "table") {
        flush();
        const rows = Array.from(element.querySelectorAll("tr"))
          .map((row) =>
            Array.from(row.querySelectorAll("th, td"))
              .map((cell) => clean(cell.textContent) ?? "")
              .join(" | "),
          )
          .filter((row) => row.replace(/[ |]/g, ""));
        if (rows.length) blocks.push(rows.join("\n"));
      } else if (tag === "br") {
        line += "\n";
      } else if (tag === "img") {
        const alt = clean(element.getAttribute("alt"));
        if (alt) line += ` [image: ${alt}] `;
      } else if (BLOCKS.has(tag)) {
        flush();
        walk(element);
        flush();
      } else {
        walk(element);
      }
    }
  };
  walk(root);
  flush();
  return blocks.join("\n\n");
}

/** Cut at a paragraph so a sentence isn't left half-read. */
function cut(text: string, max: number): string {
  if (text.length <= max) return text;
  const head = text.slice(0, max);
  const end = head.lastIndexOf("\n\n");
  return end > max * 0.6 ? head.slice(0, end) : head;
}

/** Paragraphs holding most of the query's words, the best first. */
function bestParagraphs(text: string, query: string): string[] {
  const terms = query
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter((term) => term.length > 2);
  if (!terms.length) return [];
  const scored = text
    .split(/\n\n+/)
    .map((paragraph, index) => {
      const lower = paragraph.toLowerCase();
      const score = terms.filter((term) => lower.includes(term)).length;
      return { paragraph, index, score };
    })
    .filter((entry) => entry.score > 0)
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .slice(0, 6);
  return scored.map((entry) => entry.paragraph.slice(0, 1500));
}
