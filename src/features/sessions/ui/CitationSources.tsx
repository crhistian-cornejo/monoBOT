import { openUrl } from "@tauri-apps/plugin-opener";
import { useContext, useId, useState } from "react";
import { ChevronDown, ExternalLink } from "../../../shared/ui/icons";
import { CitationContext } from "./citationContext";
import { CitationMonogram } from "./CitationMonogram";

/**
 * "N sources" under a reply: the sources it cites, numbered as the chips are.
 * Pointing at a row lights up the claims that source backs.
 */
export function CitationSources() {
  const citations = useContext(CitationContext);
  const [open, setOpen] = useState(false);
  const listId = useId();
  if (!citations || citations.sources.length === 0) return null;
  const { sources, setHovered } = citations;

  return (
    <div className="mt-3">
      <button
        type="button"
        aria-controls={listId}
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className="flex items-center gap-2 rounded-full py-1 pr-2.5 pl-1 text-[12px] text-content/70 outline-none transition-colors duration-150 hover:bg-content/8 focus-visible:ring-2 focus-visible:ring-accent/50"
      >
        <span className="flex -space-x-1">
          {sources.slice(0, 4).map((source) => (
            <CitationMonogram
              key={source.url}
              host={source.host}
              className="size-5 ring-2 ring-background-base"
            />
          ))}
        </span>
        {sources.length} {sources.length === 1 ? "source" : "sources"}
        <ChevronDown
          aria-hidden="true"
          className={`size-3.5 text-content/40 transition-transform duration-200 ease-out motion-reduce:transition-none ${
            open ? "rotate-180" : ""
          }`}
        />
      </button>
      <div
        id={listId}
        inert={!open}
        className={`grid transition-[grid-template-rows,opacity] duration-200 ease-out motion-reduce:transition-none ${
          open ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"
        }`}
      >
        <ol className="min-h-0 overflow-hidden">
          {sources.map((source) => (
            <li key={source.url} className="pt-1 first:pt-2">
              <a
                href={source.url}
                onClick={(event) => {
                  event.preventDefault();
                  void openUrl(source.url).catch((error) => {
                    console.error("Failed to open source:", error);
                  });
                }}
                onPointerEnter={() => setHovered(source.url)}
                onPointerLeave={() => setHovered(null)}
                onFocus={() => setHovered(source.url)}
                onBlur={() => setHovered(null)}
                className="group flex items-center gap-2.5 rounded-md px-2 py-1.5 text-[13px] text-content no-underline outline-none transition-colors duration-150 hover:bg-content/8 focus-visible:ring-2 focus-visible:ring-accent/50"
              >
                <span className="w-3 shrink-0 text-right text-[11px] tabular-nums text-content/40">
                  {source.number}
                </span>
                <CitationMonogram host={source.host} />
                <span className="min-w-0 flex-1 truncate">{source.title}</span>
                <span className="hidden shrink-0 text-[11px] text-content/40 sm:inline">
                  {source.host}
                </span>
                <ExternalLink
                  aria-hidden="true"
                  className="size-3.5 shrink-0 text-content/25 transition-colors duration-150 group-hover:text-content/70"
                />
              </a>
            </li>
          ))}
        </ol>
      </div>
    </div>
  );
}
