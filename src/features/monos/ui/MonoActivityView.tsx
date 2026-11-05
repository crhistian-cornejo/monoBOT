import { openUrl } from "@tauri-apps/plugin-opener";
import { useMemo, useState, type ComponentProps } from "react";
import { ChevronDown, ExternalLink, Search } from "../../../shared/ui/icons";
import { workSummaryLine } from "../../sessions/model/transcriptActivity";
import { MonoActivityTrail } from "../../sessions/ui/AgentTranscript";
import { CitationMonogram } from "../../sessions/ui/CitationMonogram";
import {
  turnResearch,
  type ResearchSource,
  type TurnResearch,
} from "../model/turnResearch";

/** How long a turn took, for the header: "42s", "3m 05s". */
export function activityDuration(ms: number | undefined): string {
  if (!ms || ms < 1000) return "";
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  return `${minutes}m ${String(seconds % 60).padStart(2, "0")}s`;
}

const plural = (count: number, one: string, many = `${one}s`) =>
  `${count} ${count === 1 ? one : many}`;

/**
 * A selected turn's legwork. The answer is in the chat, so this shows what
 * the agent did to reach it: the searches it ran, the pages it opened and
 * which of them it cited, then each step in the order it happened.
 */
export function MonoActivityView(
  trail: ComponentProps<typeof MonoActivityTrail>,
) {
  const research = useMemo(() => turnResearch(trail.blocks), [trail.blocks]);
  // A settled turn sums up its work; a live one's steps speak for themselves.
  const summary = trail.live ? "" : workSummaryLine(trail.blocks);
  const duration = trail.live
    ? ""
    : activityDuration(
        trail.blocks.find((block) => block.role === "user")?.durationMs,
      );
  // Once the answer is in, it is the chat's; the trail keeps the work.
  const blocks = useMemo(
    () =>
      trail.live || !research.answerId
        ? trail.blocks
        : trail.blocks.filter((block) => block.id !== research.answerId),
    [trail.blocks, trail.live, research.answerId],
  );
  return (
    <>
      <p className="flex min-w-0 items-center gap-1.5 px-2 pb-3 pt-1 text-[11px] leading-4 text-content/45">
        <span
          aria-hidden
          className={`size-1.5 shrink-0 rounded-full ${
            trail.live
              ? "animate-pulse bg-[var(--mono-color)]"
              : "bg-content/30"
          }`}
        />
        <span className="shrink-0 text-content/70">
          {trail.live ? "Working" : "Finished"}
        </span>
        {duration ? <span className="shrink-0">in {duration}</span> : null}
        {summary ? <span className="truncate">· {summary}</span> : null}
      </p>
      <ResearchSummary research={research} />
      <div className="px-1 pb-2">
        <MonoActivityTrail {...trail} blocks={blocks} />
      </div>
    </>
  );
}

/** The searches and the sources behind the answer, cited ones first. */
function ResearchSummary({ research }: { research: TurnResearch }) {
  const [allSources, setAllSources] = useState(false);
  const { searches, sources, skipped } = research;
  if (!searches.length && !sources.length) return null;
  const cited = sources.filter((source) => source.cited).length;
  const read = sources.filter((source) => source.read).length;
  const shown = allSources ? sources : sources.slice(0, 6);
  const stats = [
    searches.length ? plural(searches.length, "search", "searches") : "",
    read ? plural(read, "page") + " read" : "",
    cited ? `${cited} cited` : "",
    skipped ? `${skipped} passed over` : "",
  ].filter(Boolean);

  return (
    <section
      aria-label="Research"
      className="mx-1 mb-3 rounded-lg border border-content/8 bg-content/[0.02] px-2 py-2"
    >
      <p className="px-1 pb-1.5 text-[11px] tabular-nums text-content/45">
        {stats.join(" · ")}
      </p>
      {searches.length ? (
        <ul className="flex flex-col pb-1">
          {searches.map((search) => (
            <li
              key={search.id}
              className="flex min-w-0 items-center gap-2 rounded-md px-1 py-1 text-[12px] text-content/75"
            >
              <Search
                aria-hidden
                className="size-3.5 shrink-0 text-content/40"
                strokeWidth={1.75}
              />
              <span className="min-w-0 flex-1 truncate">{search.query}</span>
              {search.results ? (
                <span className="shrink-0 text-[11px] tabular-nums text-content/40">
                  {search.results}
                </span>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
      {sources.length ? (
        <ol
          className={`flex flex-col ${searches.length ? "border-t border-content/8 pt-1" : ""}`}
        >
          {shown.map((source) => (
            <SourceRow key={source.url} source={source} />
          ))}
        </ol>
      ) : null}
      {sources.length > shown.length || allSources ? (
        <button
          type="button"
          onClick={() => setAllSources((value) => !value)}
          className="mt-0.5 flex items-center gap-1 rounded-md px-1 py-1 text-[11px] text-content/50 hover:bg-content/8 hover:text-content/80"
        >
          {allSources ? "Fewer" : `${sources.length - shown.length} more`}
          <ChevronDown
            aria-hidden
            className={`size-3 transition-transform duration-200 ease-out motion-reduce:transition-none ${
              allSources ? "rotate-180" : ""
            }`}
          />
        </button>
      ) : null}
    </section>
  );
}

function SourceRow({ source }: { source: ResearchSource }) {
  return (
    <li>
      <a
        href={source.url}
        title={source.url}
        onClick={(event) => {
          event.preventDefault();
          void openUrl(source.url).catch((error) => {
            console.error("Failed to open source:", error);
          });
        }}
        className="group flex min-w-0 items-center gap-2 rounded-md px-1 py-1.5 text-[12px] no-underline outline-none transition-colors duration-150 hover:bg-content/8 focus-visible:ring-2 focus-visible:ring-accent/50"
      >
        <CitationMonogram host={source.host} />
        <span className="flex min-w-0 flex-1 flex-col">
          <span
            className={`truncate ${source.cited ? "text-content" : "text-content/70"}`}
          >
            {source.title ?? source.host}
          </span>
          {source.title ? (
            <span className="truncate text-[11px] text-content/40">
              {source.host}
            </span>
          ) : null}
        </span>
        {source.cited ? (
          <span className="shrink-0 rounded-full bg-accent/15 px-1.5 text-[10px] font-medium tabular-nums leading-4 text-accent">
            {source.cited}
          </span>
        ) : (
          <span className="shrink-0 text-[10px] text-content/35">
            {source.read ? "read" : "result"}
          </span>
        )}
        <ExternalLink
          aria-hidden
          className="size-3 shrink-0 text-content/25 transition-colors duration-150 group-hover:text-content/70"
        />
      </a>
    </li>
  );
}
