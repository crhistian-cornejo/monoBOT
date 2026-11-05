import { useEffect, useRef, useState, type ReactNode } from "react";

/**
 * Long text starts clamped, fading out, with a Show more under it, so a list
 * of write-ups stays scannable. Text that barely runs over just shows in full.
 */
export function CollapsibleBody({
  children,
  maxHeight = 180,
}: {
  children: ReactNode;
  /** The clamped height, in pixels. */
  maxHeight?: number;
}) {
  const inner = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    const el = inner.current;
    if (!el) return;
    // A little slack so a body barely over the limit just shows in full.
    const measure = () => setOverflows(el.scrollHeight > maxHeight + 48);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [maxHeight]);

  const clamped = overflows && !expanded;
  return (
    <>
      <div
        style={clamped ? { maxHeight } : undefined}
        className={
          clamped
            ? "overflow-hidden [mask-image:linear-gradient(to_bottom,black_60%,transparent)]"
            : ""
        }
      >
        <div ref={inner}>{children}</div>
      </div>
      {overflows ? (
        <button
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((current) => !current)}
          className="mt-1.5 text-[12px] text-content/50 hover:text-content"
        >
          {expanded ? "Show less" : "Show more"}
        </button>
      ) : null}
    </>
  );
}
