/**
 * Scholarly papers for the research agents, from the open APIs that need no
 * key. Each paper is a title, authors, an abstract and the open-access PDFs
 * that exist for it. The abstract is looked for in order (OpenAlex, Crossref,
 * HAL, Semantic Scholar) because publishers withhold it from some of them.
 * Network and PDF access come in through `PaperDeps`, so this stays plain.
 */

export type PaperAccess = "open" | "closed" | "unknown";

export type Paper = {
  /** DOI when there is one, else the OpenAlex or arXiv id: the cache key. */
  key: string;
  doi?: string;
  openAlexId?: string;
  title: string;
  authors: string[];
  year?: number;
  venue?: string;
  citedBy?: number;
  abstract?: string;
  /** Which source the abstract came from. */
  abstractFrom?: string;
  /**
   * Open copies to try, best first: direct PDFs, then the pages of open
   * locations, from which the app finds the PDF they point to.
   */
  pdfUrls: string[];
  /** The publisher's or repository's page for it. */
  landing?: string;
  access: PaperAccess;
};

export type PaperDeps = {
  /** GET one of the open scholarly APIs; the body as text. */
  api(url: string): Promise<string>;
};

const OPENALEX = "https://api.openalex.org";
const CROSSREF = "https://api.crossref.org";
const HAL = "https://api.archives-ouvertes.fr";
const SEMANTIC = "https://api.semanticscholar.org/graph/v1";
const ARXIV = "https://export.arxiv.org/api";
const EUROPE_PMC = "https://www.ebi.ac.uk/europepmc/webservices/rest";
const WORK_FIELDS =
  "id,doi,title,publication_year,cited_by_count,authorships,primary_location,best_oa_location,open_access,abstract_inverted_index,locations";

type Json = Record<string, unknown>;
const object = (value: unknown): Json | undefined =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Json)
    : undefined;
const list = (value: unknown): unknown[] => (Array.isArray(value) ? value : []);
const text = (value: unknown): string | undefined =>
  typeof value === "string" && value.trim() ? value.trim() : undefined;
const count = (value: unknown): number | undefined =>
  typeof value === "number" && Number.isFinite(value) ? value : undefined;

/** `10.1016/j.envsoft.2017.05.002` from a DOI, a doi.org link or `doi:` text. */
export function normalizeDoi(value: string): string | undefined {
  const match = value
    .trim()
    .match(/(?:^|doi\.org\/|doi:\s*)(10\.\d{4,9}\/[^\s"<>]+)/i);
  return match?.[1].replace(/[.,;)\]]+$/, "").toLowerCase();
}

/** An arXiv id (`2401.01234`, `hep-th/9901001`) from an id or arxiv.org link. */
export function arxivId(value: string): string | undefined {
  const match = value
    .trim()
    .match(
      /(?:arxiv\.org\/(?:abs|pdf)\/|^arxiv:\s*|^)(\d{4}\.\d{4,5}|[a-z-]+(?:\.[A-Z]{2})?\/\d{7})(?:v\d+)?(?:\.pdf)?$/i,
    );
  return match?.[1];
}

/** OpenAlex ships abstracts as {word: [positions]}; this puts them in order. */
export function abstractFromIndex(index: unknown): string | undefined {
  const words = object(index);
  if (!words) return undefined;
  const slots: string[] = [];
  for (const [word, positions] of Object.entries(words)) {
    for (const position of list(positions)) {
      if (typeof position === "number" && position >= 0 && position < 20_000)
        slots[position] = word;
    }
  }
  const joined = slots
    .filter((word) => word !== undefined)
    .join(" ")
    .trim();
  return joined.length >= 40 ? joined : undefined;
}

/** Crossref abstracts arrive as JATS XML. */
export function stripJats(raw: string): string | undefined {
  const plain = raw
    .replace(/<jats:title>[^<]*<\/jats:title>/gi, "")
    .replace(/<[^>]+>/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'")
    .replace(/\s+/g, " ")
    .replace(/^abstract\s*[:.]?\s*/i, "")
    .trim();
  return plain.length >= 40 ? plain : undefined;
}

const unique = (urls: (string | undefined)[]): string[] => [
  ...new Set(urls.filter((url): url is string => !!url)),
];

/**
 * Whether a link could lead to a paper's PDF. Indexes sometimes list a figure
 * (OpenAlex gave an article's graphical abstract, a JPG, as its "PDF"), a
 * data file or an XML copy; those never are.
 */
export function isLikelyPdfUrl(url: string): boolean {
  if (!/^https?:\/\//i.test(url)) return false;
  return !/\.(jpe?g|png|gif|svg|webp|bmp|tiff?|ico|zip|gz|tar|xml|json|csv|docx?|xlsx?|pptx?|mp4|mov)(?:[?#]|$)/i.test(
    url,
  );
}

/** The PDF an arXiv or HAL landing page implies, when its link is not one. */
export function impliedPdf(landing: string | undefined): string | undefined {
  if (!landing) return undefined;
  const arxiv = landing.match(/arxiv\.org\/abs\/([^\s?#]+)/i);
  if (arxiv) return `https://arxiv.org/pdf/${arxiv[1]}`;
  return undefined;
}

export function parseOpenAlexWork(raw: unknown): Paper | undefined {
  const work = object(raw);
  const title = text(work?.title);
  if (!work || !title) return undefined;
  const doi = normalizeDoi(text(work.doi) ?? "");
  const openAlexId = text(work.id)?.replace(/^https?:\/\/openalex\.org\//, "");
  const best = object(work.best_oa_location);
  const locations = list(work.locations).map(object);
  const primary = object(work.primary_location);
  const open = object(work.open_access);
  const abstract = abstractFromIndex(work.abstract_inverted_index);
  const landing =
    text(primary?.landing_page_url) ??
    (doi ? `https://doi.org/${doi}` : undefined);
  return {
    key: doi ?? openAlexId ?? title.toLowerCase(),
    ...(doi ? { doi } : {}),
    ...(openAlexId ? { openAlexId } : {}),
    title,
    authors: list(work.authorships).flatMap((entry) => {
      const name = text(object(object(entry)?.author)?.display_name);
      return name ? [name] : [];
    }),
    ...(count(work.publication_year)
      ? { year: count(work.publication_year) }
      : {}),
    ...(text(object(primary?.source)?.display_name)
      ? { venue: text(object(primary?.source)?.display_name) }
      : {}),
    ...(count(work.cited_by_count) !== undefined
      ? { citedBy: count(work.cited_by_count) }
      : {}),
    ...(abstract ? { abstract, abstractFrom: "OpenAlex" } : {}),
    pdfUrls: unique([
      text(best?.pdf_url),
      text(open?.oa_url),
      ...locations.map((location) => text(location?.pdf_url)),
      ...locations.map((location) =>
        impliedPdf(text(location?.landing_page_url)),
      ),
      // The pages of open locations: their PDF is found from the page.
      text(best?.landing_page_url),
      ...locations.map((location) =>
        location?.is_oa === true ? text(location?.landing_page_url) : undefined,
      ),
    ]).filter(isLikelyPdfUrl),
    ...(landing ? { landing } : {}),
    access: open?.is_oa === true ? "open" : open ? "closed" : "unknown",
  };
}

export function parseCrossrefItem(raw: unknown): Paper | undefined {
  const item = object(raw);
  const title = text(list(item?.title)[0]);
  if (!item || !title) return undefined;
  const doi = normalizeDoi(text(item.DOI) ?? "");
  const issued = list(list(object(item.issued)?.["date-parts"])[0]);
  const abstract = text(item.abstract)
    ? stripJats(String(item.abstract))
    : undefined;
  return {
    key: doi ?? title.toLowerCase(),
    ...(doi ? { doi } : {}),
    title,
    authors: list(item.author).flatMap((entry) => {
      const author = object(entry);
      const name = [text(author?.given), text(author?.family)]
        .filter(Boolean)
        .join(" ");
      return name ? [name] : [];
    }),
    ...(count(issued[0]) ? { year: count(issued[0]) } : {}),
    ...(text(list(item["container-title"])[0])
      ? { venue: text(list(item["container-title"])[0]) }
      : {}),
    ...(count(item["is-referenced-by-count"]) !== undefined
      ? { citedBy: count(item["is-referenced-by-count"]) }
      : {}),
    ...(abstract ? { abstract, abstractFrom: "Crossref" } : {}),
    // Crossref's links are the publisher's, rarely open: the open copies come
    // from the other sources.
    pdfUrls: [],
    ...(doi ? { landing: `https://doi.org/${doi}` } : {}),
    access: "unknown",
  };
}

type Offer = { abstract?: string; pdf?: string; open?: boolean };

export function parseHalDoc(raw: unknown): Offer {
  const doc = object(raw);
  const abstract = text(list(doc?.abstract_s)[0]);
  const pdf = text(doc?.fileMain_s);
  return {
    ...(abstract && abstract.length >= 40 ? { abstract } : {}),
    ...(pdf ? { pdf } : {}),
    open: doc?.openAccess_bool === true,
  };
}

async function json(deps: PaperDeps, url: string): Promise<unknown> {
  const body = await deps.api(url);
  try {
    return JSON.parse(body);
  } catch {
    throw new Error("The paper service did not answer with JSON");
  }
}

/** OpenAlex rate-limits anonymous search: its error says how long to wait. */
export class RateLimited extends Error {
  constructor(
    readonly service: string,
    readonly retryAfter?: number,
  ) {
    super(
      `${service} is rate-limiting right now${
        retryAfter ? `; retry in ${retryAfter}s` : ""
      }`,
    );
  }
}

function checkRate(service: string, data: unknown) {
  const body = object(data);
  if (body && typeof body.error === "string" && /rate/i.test(body.error))
    throw new RateLimited(service, count(body.retryAfter));
}

/**
 * Searches by words in three indexes at once: OpenAlex (Crossref when OpenAlex
 * is limiting), Semantic Scholar and arXiv. Their results are taken in turns,
 * so a preprint isn't buried under the first index's list, and a paper found
 * twice appears once.
 */
export async function findPapers(
  query: string,
  deps: PaperDeps,
  limit = 6,
  since?: number,
): Promise<{ papers: Paper[]; source: string; note?: string }> {
  const [main, semantic, arxiv] = await Promise.all([
    findInOpenAlex(query, deps, limit, since),
    searchSemanticScholar(query, deps, limit, since).catch((): Paper[] => []),
    searchArxiv(query, deps, limit, since).catch((): Paper[] => []),
  ]);
  const papers = interleave([main.papers, semantic, arxiv], limit);
  const sources = [
    main.source,
    ...(papers.some((paper) => semantic.includes(paper)) ? ["Semantic Scholar"] : []),
    ...(papers.some((paper) => arxiv.includes(paper)) ? ["arXiv"] : []),
  ];
  return {
    papers,
    source: sources.join(", "),
    ...(main.note ? { note: main.note } : {}),
  };
}

/** Takes from each list in turn, skipping a paper already taken (same DOI or title). */
export function interleave(lists: Paper[][], limit: number): Paper[] {
  const seen = new Set<string>();
  const taken: Paper[] = [];
  const longest = Math.max(0, ...lists.map((found) => found.length));
  for (let index = 0; index < longest && taken.length < limit; index++) {
    for (const found of lists) {
      const paper = found[index];
      if (!paper || taken.length >= limit) continue;
      const keys = [
        paper.doi?.toLowerCase(),
        paper.title.toLowerCase().replace(/[^\p{L}\p{N}]+/gu, " ").trim(),
      ].filter((key): key is string => !!key);
      if (keys.some((key) => seen.has(key))) continue;
      keys.forEach((key) => seen.add(key));
      taken.push(paper);
    }
  }
  return taken;
}

/** Semantic Scholar's keyword search; with no key it limits often, so it is extra. */
async function searchSemanticScholar(
  query: string,
  deps: PaperDeps,
  limit: number,
  since?: number,
): Promise<Paper[]> {
  const fields =
    "title,authors,year,venue,citationCount,externalIds,abstract,openAccessPdf,url";
  const data = object(
    await json(
      deps,
      `${SEMANTIC}/paper/search?query=${encodeURIComponent(query.trim())}&limit=${limit}&fields=${fields}${since ? `&year=${since}-` : ""}`,
    ),
  );
  return list(data?.data).flatMap((entry) => {
    const item = object(entry);
    const title = typeof item?.title === "string" ? item.title.trim() : "";
    if (!item || !title) return [];
    const ids = object(item.externalIds);
    const arxiv = typeof ids?.ArXiv === "string" ? ids.ArXiv : undefined;
    const doi =
      normalizeDoi(typeof ids?.DOI === "string" ? ids.DOI : "") ??
      (arxiv ? `10.48550/arxiv.${arxiv.toLowerCase()}` : undefined);
    const pdf = object(item.openAccessPdf)?.url;
    const paper: Paper = {
      key: doi ?? `s2:${String(item.paperId ?? title)}`,
      ...(doi ? { doi } : {}),
      title,
      authors: list(item.authors)
        .map((author) => object(author)?.name)
        .filter((name): name is string => typeof name === "string"),
      ...(typeof item.year === "number" ? { year: item.year } : {}),
      ...(typeof item.venue === "string" && item.venue ? { venue: item.venue } : {}),
      ...(typeof item.citationCount === "number"
        ? { citedBy: item.citationCount }
        : {}),
      ...(typeof item.abstract === "string" && item.abstract
        ? { abstract: item.abstract, abstractFrom: "Semantic Scholar" }
        : {}),
      pdfUrls: typeof pdf === "string" && pdf ? [pdf] : [],
      ...(typeof item.url === "string" ? { landing: item.url } : {}),
      access: typeof pdf === "string" && pdf ? "open" : "unknown",
    };
    return [paper];
  });
}

/** arXiv's own search (Atom): preprints, always open, often newer than journals. */
async function searchArxiv(
  query: string,
  deps: PaperDeps,
  limit: number,
  since?: number,
): Promise<Paper[]> {
  const words = query
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((word) => `all:${encodeURIComponent(word.replace(/["()]/g, ""))}`)
    .join("+AND+");
  const atom = await deps.api(
    `${ARXIV}/query?search_query=${words}&start=0&max_results=${limit}&sortBy=relevance`,
  );
  return parseArxivFeed(atom).filter(
    (paper) => !since || !paper.year || paper.year >= since,
  );
}

/** The entries of an arXiv Atom feed as papers. */
export function parseArxivFeed(atom: string): Paper[] {
  const tag = (block: string, name: string) =>
    block
      .match(new RegExp(`<${name}[^>]*>([\\s\\S]*?)</${name}>`))?.[1]
      ?.replace(/\s+/g, " ")
      .trim();
  return Array.from(atom.matchAll(/<entry>([\s\S]*?)<\/entry>/g)).flatMap(
    ([, entry]) => {
      const id = arxivId(tag(entry, "id") ?? "");
      const title = tag(entry, "title");
      if (!id || !title) return [];
      const doi = `10.48550/arxiv.${id.toLowerCase()}`;
      const published = tag(entry, "published");
      const summary = tag(entry, "summary");
      return [
        {
          key: doi,
          doi,
          title: decodeXml(title),
          authors: Array.from(
            entry.matchAll(/<author>\s*<name>([\s\S]*?)<\/name>/g),
          ).map(([, name]) => decodeXml(name.trim())),
          ...(published ? { year: Number(published.slice(0, 4)) } : {}),
          venue: "arXiv",
          ...(summary
            ? { abstract: decodeXml(summary), abstractFrom: "arXiv" }
            : {}),
          pdfUrls: [`https://arxiv.org/pdf/${id}`],
          landing: `https://arxiv.org/abs/${id}`,
          access: "open" as const,
        },
      ];
    },
  );
}

function decodeXml(value: string): string {
  return value
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

async function findInOpenAlex(
  query: string,
  deps: PaperDeps,
  limit: number,
  since?: number,
): Promise<{ papers: Paper[]; source: string; note?: string }> {
  const terms = encodeURIComponent(query.trim());
  const filter = since ? `&filter=from_publication_date:${since}-01-01` : "";
  try {
    const data = await json(
      deps,
      `${OPENALEX}/works?search=${terms}&per-page=${limit}&select=${WORK_FIELDS}${filter}`,
    );
    checkRate("OpenAlex", data);
    const papers = list(object(data)?.results).flatMap((work) => {
      const paper = parseOpenAlexWork(work);
      return paper ? [paper] : [];
    });
    if (papers.length) return { papers, source: "OpenAlex" };
  } catch {
    // Rate-limited or down: Crossref answers below.
  }
  const data = await json(
    deps,
    `${CROSSREF}/works?query.bibliographic=${terms}&rows=${limit}${
      since ? `&filter=from-pub-date:${since}` : ""
    }`,
  );
  const papers = list(object(object(data)?.message)?.items).flatMap((item) => {
    const paper = parseCrossrefItem(item);
    return paper ? [paper] : [];
  });
  return {
    papers,
    source: "Crossref",
    note: "OpenAlex was not available, so these come from Crossref: no citation graph, and abstracts are often missing until you read a paper.",
  };
}

/** A reference to one paper: a DOI, an arXiv id or link, or an OpenAlex id. */
export type PaperRef =
  | { doi: string }
  /** A paper kept from a PDF the user gave, with no DOI to look it up by. */
  | { cached: string }
  | { arxiv: string }
  | { openAlex: string }
  | { title: string };

export function parseRef(value: string): PaperRef {
  const local = value.trim().match(/^pdf:[0-9a-f]{8,64}$/i);
  if (local) return { cached: local[0].toLowerCase() };
  const doi = normalizeDoi(value);
  if (doi) return { doi };
  const arxiv = arxivId(value);
  if (arxiv) return { arxiv };
  const alex = value.trim().match(/(?:openalex\.org\/)?(W\d{4,})$/i);
  if (alex) return { openAlex: alex[1].toUpperCase() };
  return { title: value.trim() };
}

async function openAlexFor(
  ref: PaperRef,
  deps: PaperDeps,
): Promise<Paper | undefined> {
  let path: string;
  if ("cached" in ref) return undefined;
  if ("doi" in ref) path = `/works/doi:${ref.doi}`;
  else if ("openAlex" in ref) path = `/works/${ref.openAlex}`;
  else if ("arxiv" in ref) path = `/works/doi:10.48550/arxiv.${ref.arxiv}`;
  else {
    const found = await findPapers(ref.title, deps, 1);
    return found.papers[0];
  }
  const data = await json(deps, `${OPENALEX}${path}?select=${WORK_FIELDS}`);
  checkRate("OpenAlex", data);
  return parseOpenAlexWork(data);
}

async function crossrefFor(
  doi: string,
  deps: PaperDeps,
): Promise<Paper | undefined> {
  const data = await json(deps, `${CROSSREF}/works/${encodeURIComponent(doi)}`);
  return parseCrossrefItem(object(data)?.message);
}

async function halFor(doi: string, deps: PaperDeps): Promise<Offer> {
  const data = await json(
    deps,
    `${HAL}/search/?q=${encodeURIComponent(`doiId_s:"${doi}"`)}&fl=abstract_s,fileMain_s,openAccess_bool&rows=1&wt=json`,
  );
  return parseHalDoc(list(object(object(data)?.response)?.docs)[0]);
}

/** Europe PMC holds the open full text of most biomedical and some other papers. */
export function parseEuropePmc(raw: unknown): Offer {
  const entry = object(list(object(object(raw)?.resultList)?.result)[0]);
  const pmcid = text(entry?.pmcid);
  const open = entry?.isOpenAccess === "Y";
  const abstract = text(entry?.abstractText)
    ? stripJats(String(entry?.abstractText))
    : undefined;
  return {
    ...(abstract ? { abstract } : {}),
    ...(open && pmcid
      ? {
          pdf: `https://europepmc.org/backend/ptpmcrender.fcgi?accid=${pmcid}&blobtype=pdf`,
          open: true,
        }
      : {}),
  };
}

async function europePmcFor(doi: string, deps: PaperDeps): Promise<Offer> {
  return parseEuropePmc(
    await json(
      deps,
      `${EUROPE_PMC}/search?query=${encodeURIComponent(`DOI:${doi}`)}&resultType=core&format=json&pageSize=1`,
    ),
  );
}

async function semanticFor(
  doi: string,
  deps: PaperDeps,
): Promise<{ abstract?: string; pdf?: string }> {
  const data = object(
    await json(
      deps,
      `${SEMANTIC}/paper/DOI:${encodeURIComponent(doi)}?fields=abstract,openAccessPdf`,
    ),
  );
  const abstract = text(data?.abstract);
  const pdf = text(object(data?.openAccessPdf)?.url);
  return {
    ...(abstract && abstract.length >= 40 ? { abstract } : {}),
    ...(pdf ? { pdf } : {}),
  };
}

const quiet = async <T>(job: () => Promise<T>): Promise<T | undefined> => {
  try {
    return await job();
  } catch {
    return undefined;
  }
};

/**
 * Everything the open APIs know about one paper: metadata, then the abstract
 * from whichever source has it, then every open PDF that exists.
 */
export async function resolvePaper(
  ref: PaperRef,
  deps: PaperDeps,
): Promise<Paper | undefined> {
  let paper = await quiet(() => openAlexFor(ref, deps));
  const doi = paper?.doi ?? ("doi" in ref ? ref.doi : undefined);
  if (!paper && doi) paper = await quiet(() => crossrefFor(doi, deps));
  if (!paper) return undefined;
  const found: Paper = { ...paper, pdfUrls: [...paper.pdfUrls] };
  if (!found.doi && doi) found.doi = doi;
  if (!doi) return found;
  const [crossref, hal, semantic, pmc] = await Promise.all([
    found.abstract ? undefined : quiet(() => crossrefFor(doi, deps)),
    quiet(() => halFor(doi, deps)),
    found.abstract && found.pdfUrls.length
      ? undefined
      : quiet(() => semanticFor(doi, deps)),
    found.pdfUrls.length ? undefined : quiet(() => europePmcFor(doi, deps)),
  ]);
  const abstracts: [string | undefined, string][] = [
    [crossref?.abstract, "Crossref"],
    [hal?.abstract, "HAL"],
    [pmc?.abstract, "Europe PMC"],
    [semantic?.abstract, "Semantic Scholar"],
  ];
  if (!found.abstract) {
    const hit = abstracts.find(([abstract]) => abstract);
    if (hit?.[0]) {
      found.abstract = hit[0];
      found.abstractFrom = hit[1];
    }
  }
  found.pdfUrls = unique([
    ...found.pdfUrls,
    hal?.pdf,
    pmc?.pdf,
    semantic?.pdf,
  ]).filter(isLikelyPdfUrl);
  if (hal?.open || pmc?.open || (semantic?.pdf && isLikelyPdfUrl(semantic.pdf)))
    found.access = "open";
  if (!found.venue && crossref?.venue) found.venue = crossref.venue;
  return found;
}

/** The first author and year, as papers are named in text: "Coron et al. (2017)". */
export function paperLabel(paper: Pick<Paper, "authors" | "year">): string {
  const family = (name: string) => {
    const words = name.trim().split(/\s+/);
    return words[words.length - 1] || name;
  };
  const [first, second] = paper.authors;
  const who = !first
    ? "Unknown"
    : paper.authors.length === 1
      ? family(first)
      : paper.authors.length === 2
        ? `${family(first)} & ${family(second)}`
        : `${family(first)} et al.`;
  return paper.year ? `${who} (${paper.year})` : who;
}
