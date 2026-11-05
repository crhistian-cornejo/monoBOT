import { useCallback, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { ChevronLeft, ChevronRight } from "./icons";
import { revealScrollLeft, stripOverflow, type StripOverflow } from "./stripOverflow";

/** How far into each edge the fade reaches, and how clear of it the active item stays. */
const FADE = 28;

/**
 * A row that scrolls sideways when its items don't fit: the cut edges fade out
 * and carry an arrow to move along. The item marked `data-active` is kept in
 * view, so the strip opens at its start rather than part-way through.
 */
export function ScrollStrip({
  children,
  activeKey,
  label = "items",
  className = "",
}: {
  children: ReactNode;
  /** Changes when the active item changes, to bring it into view. */
  activeKey?: string;
  /** What the arrows scroll, for their tooltips: "Scroll {label} left". */
  label?: string;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [overflow, setOverflow] = useState<StripOverflow>({ left: false, right: false });

  const sync = useCallback(() => {
    const el = ref.current;
    if (!el) return;
    const next = stripOverflow(el.scrollLeft, el.clientWidth, el.scrollWidth);
    setOverflow((prev) =>
      prev.left === next.left && prev.right === next.right ? prev : next,
    );
  }, []);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    sync();
    el.addEventListener("scroll", sync, { passive: true });
    el.addEventListener("scrollend", sync, { passive: true });
    const observer = new ResizeObserver(sync);
    observer.observe(el);
    for (const child of Array.from(el.children)) observer.observe(child);
    return () => {
      el.removeEventListener("scroll", sync);
      el.removeEventListener("scrollend", sync);
      observer.disconnect();
    };
  }, [sync]);

  // Set the strip's own scroll rather than scrollIntoView, which would also
  // scroll every pane around it.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const active = el.querySelector<HTMLElement>("[data-active]");
    el.scrollLeft = active
      ? revealScrollLeft(active.offsetLeft, active.offsetWidth, el.scrollLeft, el.clientWidth, FADE)
      : 0;
    sync();
  }, [activeKey, sync]);

  const scrollBy = (direction: -1 | 1) => {
    const el = ref.current;
    if (!el) return;
    el.scrollBy({ left: direction * Math.max(el.clientWidth * 0.6, 96), behavior: "smooth" });
    // A smooth scroll the page does not paint (a hidden or background view)
    // may land without scroll events: look again once it has.
    window.setTimeout(sync, 450);
  };

  const left = overflow.left ? "transparent 0" : "#000 0";
  const right = overflow.right ? "transparent 100%" : "#000 100%";
  const mask = `linear-gradient(to right, ${left}, #000 ${FADE}px, #000 calc(100% - ${FADE}px), ${right})`;

  return (
    <div className="relative min-w-0">
      <div
        ref={ref}
        className={`scrollbar-none relative flex items-center overflow-x-auto overflow-y-hidden overscroll-x-contain ${className}`}
        style={{ maskImage: mask, WebkitMaskImage: mask }}
      >
        {children}
      </div>
      {overflow.left ? <StripArrow side="left" label={label} onClick={() => scrollBy(-1)} /> : null}
      {overflow.right ? <StripArrow side="right" label={label} onClick={() => scrollBy(1)} /> : null}
    </div>
  );
}

function StripArrow({
  side,
  label,
  onClick,
}: {
  side: "left" | "right";
  label: string;
  onClick: () => void;
}) {
  const title = `Scroll ${label} ${side}`;
  const Icon = side === "left" ? ChevronLeft : ChevronRight;
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      tabIndex={-1}
      onClick={onClick}
      className={`absolute top-1/2 grid size-6 -translate-y-1/2 place-items-center rounded-md bg-content/10 text-content/70 backdrop-blur-xl hover:bg-content/15 hover:text-content ${
        side === "left" ? "left-1" : "right-1"
      }`}
    >
      <Icon className="size-3.5" strokeWidth={1.75} />
    </button>
  );
}
