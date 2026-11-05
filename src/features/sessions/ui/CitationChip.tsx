import { openUrl } from "@tauri-apps/plugin-opener";
import { useContext, useEffect, useId, useRef, useState } from "react";
import { citationHost, parseCitationTitle } from "../model/citations";
import { focusPaperPassage, openSource } from "../../papers/model/openPaper";
import { recordPaperPassage } from "../../papers/model/paperCitations";
import { CitationContext } from "./citationContext";
import { CitationMonogram } from "./CitationMonogram";
import type { GroupedCitation } from "./citationGroups";

// The hover timing, the shared preview and the lit-up claim follow KokonutUI's
// AI Inline Citations (MIT, https://kokonutui.com), rebuilt on the shared
// Popover and the app's tokens.

// Hosts that only resolve to someone else's page: their name tells nothing.
const RESOLVERS = new Set([
  "doi.org",
  "dx.doi.org",
  "hdl.handle.net",
  "n2t.net",
]);

/** The chip's words: the site, or the work's first words for a DOI link. */
export function chipLabel(host: string, title: string): string {
  if (!RESOLVERS.has(host) || !title || title === host) return host;
  const words = title.split(/\s+/).slice(0, 3).join(" ");
  return words.length > 22 ? `${words.slice(0, 21).trimEnd()}…` : words;
}

/**
 * A source cited inline: a small chip with the site's name. Hovering or
 * focusing it previews the source and lights up the claim it backs; clicking
 * opens the page.
 */
export function CitationChip({
  href,
  number: labelNumber,
  title,
  dir,
  more = [],
}: {
  href: string;
  number: number;
  title?: string;
  dir?: string;
  /** Sources cited right after this one, for the same claim: `+N` on the chip. */
  more?: GroupedCitation[];
}) {
  const citations = useContext(CitationContext);
  const [el, setEl] = useState<HTMLAnchorElement | null>(null);
  const id = useId();
  const host = citationHost(href);
  const source = parseCitationTitle(title, href);
  const number = citations?.numberOf(href) ?? labelNumber;
  const register = citations?.register;

  // Resting on a citation of a paper that is open beside the chat points that
  // paper at the cited passage; passing over it quickly does nothing.
  const pointing = useRef<number | undefined>(undefined);
  const pointAtPaper = () => {
    window.clearTimeout(pointing.current);
    pointing.current = window.setTimeout(
      () => focusPaperPassage(href, source.snippet),
      160,
    );
  };
  useEffect(() => () => window.clearTimeout(pointing.current), []);

  // A paper's page remembers what answers cited from it, to underline it all.
  const snippet = source.snippet;
  useEffect(
    () => recordPaperPassage(href, snippet, number),
    [href, snippet, number],
  );

  const moreKey = more.map((entry) => entry.href).join("\n");
  useEffect(() => {
    if (!el || !register) return;
    // The chip stands for every source in it, so each one's row lights it.
    const stops = [href, ...moreKey.split("\n").filter(Boolean)].map((url) =>
      register(url, el),
    );
    return () => stops.forEach((stop) => stop());
  }, [el, href, moreKey, register]);

  const previewing = !!el && citations?.preview?.el === el;
  const active =
    previewing ||
    (!!citations?.hovered &&
      (citations.hovered === href ||
        more.some((entry) => entry.href === citations.hovered)));
  const preview = { id, url: href, title, more };

  return (
    <a
      ref={setEl}
      href={href}
      data-citation=""
      dir={dir ?? "auto"}
      aria-describedby={previewing ? citations?.previewId : undefined}
      aria-label={
        more.length
          ? `Sources ${[number, ...more.map((entry) => citations?.numberOf(entry.href) ?? "")].join(", ")}: ${source.title} and ${more.length} more`
          : `Source ${number}: ${source.title}`
      }
      className={`mx-0.5 inline-flex h-5 -translate-y-px items-center gap-1 rounded-full py-0 pr-1.5 pl-0.5 align-middle text-[11px] font-medium no-underline outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-accent/50 ${
        active
          ? "bg-content text-background-base"
          : "bg-content/8 text-content/70 hover:bg-content/12"
      }`}
      onClick={(event) => {
        event.preventDefault();
        citations?.hide(true);
        void openSource(href, source.snippet, openUrl).catch((error) => {
          console.error("Failed to open source:", error);
        });
      }}
      onFocus={() => {
        if (el) citations?.show({ ...preview, el }, true);
        pointAtPaper();
      }}
      onBlur={() => {
        citations?.hide(true);
        window.clearTimeout(pointing.current);
      }}
      onPointerEnter={(event) => {
        if (el && event.pointerType === "mouse") {
          citations?.show({ ...preview, el }, false);
          pointAtPaper();
        }
      }}
      onPointerLeave={(event) => {
        if (event.pointerType === "mouse") {
          citations?.hide(false);
          window.clearTimeout(pointing.current);
        }
      }}
    >
      <CitationMonogram host={host} inverted={active} />
      {chipLabel(host, source.title)}
      {more.length ? <span className="opacity-60">+{more.length}</span> : null}
    </a>
  );
}
