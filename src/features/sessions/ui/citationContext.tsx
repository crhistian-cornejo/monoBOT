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
import {
  collectCitations,
  parseCitationTitle,
  type CitationSource,
} from "../model/citations";
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
      if (immediate || open.current || Date.now() - lastClosed.current < WARM_MS) {
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

  const known = preview
    ? sources.find((source) => source.url === preview.url)
    : undefined;
  const card = preview
    ? {
        ...known,
        ...(preview.title
          ? parseCitationTitle(preview.title, preview.url)
          : {}),
      }
    : null;

  return (
    <CitationContext.Provider value={value}>
      {children}
      {preview && card ? (
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
            <div className="p-3">
              <div className="flex items-center gap-1.5 text-[11px] text-content/55">
                <CitationMonogram host={known?.host ?? preview.url} />
                <span className="min-w-0 flex-1 truncate">
                  {known?.host ?? preview.url}
                </span>
                {known ? (
                  <span className="shrink-0 tabular-nums">{known.number}</span>
                ) : null}
                <ExternalLink className="size-3 shrink-0" aria-hidden="true" />
              </div>
              <p className="mt-1.5 text-[13px] font-medium leading-snug text-content">
                {card.title ?? known?.host}
              </p>
              {card.snippet ? (
                <p className="mt-1 line-clamp-3 text-[12px] leading-relaxed text-content/60">
                  {card.snippet}
                </p>
              ) : null}
            </div>
          </FadeIn>
        </Popover>
      ) : null}
    </CitationContext.Provider>
  );
}
