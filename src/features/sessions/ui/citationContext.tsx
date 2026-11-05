import {
  createContext,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { ExternalLink } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import { PaperPassagePreview } from "../../papers/ui/PaperPassagePreview";
import {
  citationHost,
  collectCitations,
  parseCitationTitle,
  type CitationSource,
} from "../model/citations";
import {
  fetchLinkPreviewMetadata,
  type LinkPreviewMetadata,
} from "../model/linkPreview";
import { CitationMonogram } from "./CitationMonogram";
import { claimRange, paintClaims } from "./citationClaim";

const CARD_WIDTH = 288;
const OPEN_DELAY_MS = 120;
const CLOSE_DELAY_MS = 90;
/** After a preview closes, the next chip's preview opens at once inside this window. */
const WARM_MS = 400;

export type CitationPreview = {
  /** Tells one chip from another that cites the same source. */
  id: string;
  el: HTMLElement;
  url: string;
  title?: string;
  /** A plain web link rather than a numbered citation: the card reads the page. */
  link?: boolean;
  /** Sources cited beside this one for the same claim; the card lists them all. */
  more?: { href: string; title?: string }[];
};

export type CitationState = {
  sources: CitationSource[];
  numberOf: (url: string) => number | undefined;
  preview: CitationPreview | null;
  previewId: string;
  /** The source whose row the pointer is on in the sources list. */
  hovered: string | null;
  setHovered: (url: string | null) => void;
  show: (next: CitationPreview, immediate: boolean) => void;
  hide: (immediate: boolean) => void;
  register: (url: string, el: HTMLElement) => () => void;
};

/** Null outside a text that opted into citations, where numbered links stay links. */
export const CitationContext = createContext<CitationState | null>(null);

/** Fades its content in once mounted, so a preview that changes source cross-fades. */
function FadeIn({ children }: { children: ReactNode }) {
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const frame = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(frame);
  }, []);
  return (
    <div
      className={`transition-opacity duration-150 ease-out motion-reduce:transition-none ${
        shown ? "opacity-100" : "opacity-0"
      }`}
    >
      {children}
    </div>
  );
}

/** Where a link points, shortened for a card: the path without the scheme. */
export function linkPathLabel(url: string): string {
  try {
    const parsed = new URL(url);
    const path = `${parsed.pathname === "/" ? "" : parsed.pathname}${parsed.search}`;
    return decodeURI(`${parsed.hostname.replace(/^www\./, "")}${path}`);
  } catch {
    return url;
  }
}

/** A plain link's card: the page's icon, title and summary once they load. */
function LinkCard({ url }: { url: string }) {
  const [meta, setMeta] = useState<LinkPreviewMetadata | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    setMeta(null);
    setFailed(false);
    fetchLinkPreviewMetadata(url)
      .then((next) => live && setMeta(next))
      .catch(() => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [url]);
  const loading = !meta && !failed;
  return (
    <div className="p-3">
      <div className="flex items-center gap-1.5 text-[11px] text-content/55">
        {meta?.faviconDataUrl ? (
          <img
            src={meta.faviconDataUrl}
            alt=""
            className="size-4 shrink-0 rounded-sm"
          />
        ) : (
          <CitationMonogram host={url} />
        )}
        <span className="min-w-0 flex-1 truncate">{linkPathLabel(url)}</span>
        <ExternalLink className="size-3 shrink-0" aria-hidden="true" />
      </div>
      {loading ? (
        <div aria-hidden className="mt-2 space-y-1.5">
          <div className="h-3 w-4/5 animate-pulse rounded bg-content/8" />
          <div className="h-2.5 w-3/5 animate-pulse rounded bg-content/6" />
        </div>
      ) : meta?.title ? (
        <>
          <p className="mt-1.5 line-clamp-2 text-[13px] font-medium leading-snug text-content">
            {meta.title}
          </p>
          {meta.description ? (
            <p className="mt-1 line-clamp-3 text-[12px] leading-relaxed text-content/60">
              {meta.description}
            </p>
          ) : null}
        </>
      ) : (
        <p className="mt-1.5 text-[12px] text-content/50">
          Opens in your browser
        </p>
      )}
    </div>
  );
}

/** One cited source in the preview: site, number, title and the passage. */
function SourceCard({
  url,
  title,
  sources,
}: {
  url: string;
  title?: string;
  sources: CitationSource[];
}) {
  const known = sources.find((source) => source.url === url);
  const card = { ...known, ...(title ? parseCitationTitle(title, url) : {}) };
  const host = known?.host ?? citationHost(url);
  return (
    <div className="p-3">
      <div className="flex items-center gap-1.5 text-[11px] text-content/55">
        <CitationMonogram host={host} />
        <span className="min-w-0 flex-1 truncate">{host}</span>
        {known ? (
          <span className="shrink-0 tabular-nums">{known.number}</span>
        ) : null}
        <ExternalLink className="size-3 shrink-0" aria-hidden="true" />
      </div>
      <p className="mt-1.5 text-[13px] font-medium leading-snug text-content">
        {card.title ?? host}
      </p>
      {card.snippet ? (
        <p className="mt-1 line-clamp-3 text-[12px] leading-relaxed text-content/60">
          {card.snippet}
        </p>
      ) : null}
      {/* A paper's passage also shows where it is on the page. */}
      <PaperPassagePreview
        href={url}
        passage={card.snippet}
        width={CARD_WIDTH - 24}
      />
    </div>
  );
}

/** Gives a text its citations only when it opted in; otherwise it renders as is. */
export function CitationScope({
  text,
  enabled,
  children,
}: {
  text: string;
  enabled: boolean;
  children: ReactNode;
}) {
  return enabled ? (
    <CitationProvider text={text}>{children}</CitationProvider>
  ) : (
    <>{children}</>
  );
}

/**
 * Numbers the sources a text cites, owns the one preview card every chip
 * shares, and paints the claim a chip or a source row points at.
 */
export function CitationProvider({
  text,
  children,
}: {
  text: string;
  children: ReactNode;
}) {
  const sources = useMemo(() => collectCitations(text), [text]);
  const numbers = useMemo(
    () => new Map(sources.map((source) => [source.url, source.number])),
    [sources],
  );
  const numberOf = useCallback((url: string) => numbers.get(url), [numbers]);
  const [preview, setPreview] = useState<CitationPreview | null>(null);
  const [hovered, setHovered] = useState<string | null>(null);
  const previewId = useId();
  const chips = useRef(new Map<string, Set<HTMLElement>>());
  const openTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const closeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastClosed = useRef(0);
  const open = useRef(false);

  const clear = useCallback(() => {
    if (openTimer.current) clearTimeout(openTimer.current);
    if (closeTimer.current) clearTimeout(closeTimer.current);
  }, []);

  const show = useCallback(
    (next: CitationPreview, immediate: boolean) => {
      clear();
      if (
        immediate ||
        open.current ||
        Date.now() - lastClosed.current < WARM_MS
      ) {
        open.current = true;
        setPreview(next);
        return;
      }
      openTimer.current = setTimeout(() => {
        open.current = true;
        setPreview(next);
      }, OPEN_DELAY_MS);
    },
    [clear],
  );

  const hide = useCallback(
    (immediate: boolean) => {
      clear();
      const close = () => {
        open.current = false;
        lastClosed.current = Date.now();
        setPreview(null);
      };
      if (immediate) close();
      else closeTimer.current = setTimeout(close, CLOSE_DELAY_MS);
    },
    [clear],
  );

  const register = useCallback((url: string, el: HTMLElement) => {
    const set = chips.current.get(url) ?? new Set<HTMLElement>();
    set.add(el);
    chips.current.set(url, set);
    return () => {
      set.delete(el);
    };
  }, []);

  useEffect(() => clear, [clear]);

  // The claim of the chip being previewed, or of every chip citing the source
  // whose row the pointer is on.
  useEffect(() => {
    const ranges: Range[] = [];
    const targets = preview
      ? [preview.el]
      : hovered
        ? [...(chips.current.get(hovered) ?? [])]
        : [];
    for (const el of targets) {
      const range = claimRange(el);
      if (range) ranges.push(range);
    }
    paintClaims(ranges);
    return () => paintClaims([]);
  }, [preview, hovered]);

  const value = useMemo<CitationState>(
    () => ({
      sources,
      numberOf,
      preview,
      previewId,
      hovered,
      setHovered,
      show,
      hide,
      register,
    }),
    [sources, numberOf, preview, previewId, hovered, show, hide, register],
  );

  return (
    <CitationContext.Provider value={value}>
      {children}
      {preview ? (
        <Popover
          anchor={preview.el}
          side="top"
          align="center"
          gap={6}
          width={CARD_WIDTH}
          constrainHeight={false}
          id={previewId}
          role="tooltip"
          className="pointer-events-none"
          onDismiss={() => hide(true)}
        >
          <FadeIn key={preview.id}>
            {preview.link ? (
              <LinkCard url={preview.url} />
            ) : (
              <div className="divide-y divide-content/8">
                {[
                  { url: preview.url, title: preview.title },
                  ...(preview.more ?? []).map((entry) => ({
                    url: entry.href,
                    title: entry.title,
                  })),
                ].map((entry) => (
                  <SourceCard
                    key={entry.url}
                    url={entry.url}
                    title={entry.title}
                    sources={sources}
                  />
                ))}
              </div>
            )}
          </FadeIn>
        </Popover>
      ) : null}
    </CitationContext.Provider>
  );
}
