import { openUrl } from "@tauri-apps/plugin-opener";
import { useContext, useEffect, useId, useState } from "react";
import { citationHost, parseCitationTitle } from "../model/citations";
import { CitationContext } from "./citationContext";
import { CitationMonogram } from "./CitationMonogram";

// The hover timing, the shared preview and the lit-up claim follow KokonutUI's
// AI Inline Citations (MIT, https://kokonutui.com), rebuilt on the shared
// Popover and the app's tokens.

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
}: {
  href: string;
  number: number;
  title?: string;
  dir?: string;
}) {
  const citations = useContext(CitationContext);
  const [el, setEl] = useState<HTMLAnchorElement | null>(null);
  const id = useId();
  const host = citationHost(href);
  const source = parseCitationTitle(title, href);
  const number = citations?.numberOf(href) ?? labelNumber;
  const register = citations?.register;

  useEffect(() => {
    if (!el || !register) return;
    return register(href, el);
  }, [el, href, register]);

  const previewing = !!el && citations?.preview?.el === el;
  const active = previewing || citations?.hovered === href;

  return (
    <a
      ref={setEl}
      href={href}
      data-citation=""
      dir={dir ?? "auto"}
      aria-describedby={previewing ? citations?.previewId : undefined}
      aria-label={`Source ${number}: ${source.title}`}
      className={`mx-0.5 inline-flex h-5 -translate-y-px items-center gap-1 rounded-full py-0 pr-1.5 pl-0.5 align-middle text-[11px] font-medium no-underline outline-none transition-colors duration-150 focus-visible:ring-2 focus-visible:ring-accent/50 ${
        active
          ? "bg-content text-background-base"
          : "bg-content/8 text-content/70 hover:bg-content/12"
      }`}
      onClick={(event) => {
        event.preventDefault();
        citations?.hide(true);
        void openUrl(href).catch((error) => {
          console.error("Failed to open source:", error);
        });
      }}
      onFocus={() => el && citations?.show({ id, el, url: href, title }, true)}
      onBlur={() => citations?.hide(true)}
      onPointerEnter={(event) => {
        if (el && event.pointerType === "mouse") {
          citations?.show({ id, el, url: href, title }, false);
        }
      }}
      onPointerLeave={(event) => {
        if (event.pointerType === "mouse") citations?.hide(false);
      }}
    >
      <CitationMonogram host={host} inverted={active} />
      {host}
    </a>
  );
}
