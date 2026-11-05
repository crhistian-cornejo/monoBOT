import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { suppressTextSelection } from "../lib/drag";
import { beginLayoutTransition } from "../lib/layoutTransition";
import {
  alongHandle,
  clearResizeLimit,
  paintResizeLimit,
} from "../lib/resizeLimit";

type Options = {
  min: number;
  direction?: "left" | "right";
  max: () => number;
  defaultWidth: number;
  initial: number;
  onCommit?: (width: number) => void;
};

function clampTo(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, Math.round(value)));
}

/** Drag a pane's width by writing the DOM directly so React re-renders can't fight the cursor. */
export function useDragResize({
  direction = "right",
  min,
  max,
  defaultWidth,
  initial,
  onCommit,
}: Options) {
  const minRef = useRef(min);
  minRef.current = min;
  const maxRef = useRef(max);
  maxRef.current = max;
  const onCommitRef = useRef(onCommit);
  onCommitRef.current = onCommit;
  const defaultRef = useRef(defaultWidth);
  defaultRef.current = defaultWidth;

  const clamp = useCallback((value: number) => {
    return clampTo(value, minRef.current, maxRef.current());
  }, []);

  const [width, setWidth] = useState(() => clamp(initial));
  const [dragging, setDragging] = useState(false);
  const paneRef = useRef<HTMLElement | null>(null);
  const widthRef = useRef(width);
  /** The width the user chose. A narrow window shows less, then gives it back. */
  const preferredRef = useRef(initial);
  const stopDrag = useRef<(() => void) | null>(null);

  const apply = (next: number) => {
    widthRef.current = next;
    const pane = paneRef.current;
    if (pane) pane.style.width = `${next}px`;
  };

  const setPaneRef = useCallback((el: HTMLElement | null) => {
    paneRef.current = el;
    if (el) el.style.width = `${widthRef.current}px`;
  }, []);

  const commit = (next: number) => {
    const value = clamp(next);
    preferredRef.current = value;
    apply(value);
    setWidth(value);
    onCommitRef.current?.(value);
  };

  // `max` depends on the window. Without this, a pane widened in a large
  // window kept its width when the window shrank, and spilled past its edge
  // with its own close button.
  useEffect(() => {
    let frame = 0;
    const fit = () => {
      frame = 0;
      if (stopDrag.current) return;
      const value = clamp(preferredRef.current);
      if (value === widthRef.current) return;
      apply(value);
      setWidth(value);
    };
    const onResize = () => {
      if (!frame) frame = requestAnimationFrame(fit);
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [clamp]);

  const onPointerDown = (event: ReactPointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget;
    const pointerId = event.pointerId;
    const startX = event.clientX;
    const startW = widthRef.current;
    handle.setPointerCapture(pointerId);
    setDragging(true);
    const restoreSelection = suppressTextSelection();
    const previousCursor = document.body.style.cursor;
    document.body.style.cursor = "col-resize";
    document.documentElement.classList.add("is-resizing");
    const endTransition = beginLayoutTransition();

    // Write once per frame: each width change lays out the panes beside it.
    let frame = 0;
    let pendingX = startX;
    let pendingY = event.clientY;
    const sign = direction === "left" ? -1 : 1;
    const onMove = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      pendingX = ev.clientX;
      pendingY = ev.clientY;
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        const wanted = startW + (pendingX - startX) * sign;
        const next = clamp(wanted);
        apply(next);
        // Past min or max, the handle glows instead of just going still.
        paintResizeLimit(
          handle,
          "x",
          (wanted - next) * sign,
          alongHandle(handle, "x", pendingX, pendingY),
        );
      });
    };

    const stop = () => {
      if (stopDrag.current !== stop) return;
      stopDrag.current = null;
      if (frame) cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("pointercancel", onUp);
      restoreSelection();
      clearResizeLimit(handle);
      document.body.style.cursor = previousCursor;
      document.documentElement.classList.remove("is-resizing");
      setDragging(false);
      try {
        handle.releasePointerCapture(pointerId);
      } catch {
        /* already released */
      }
      commit(startW + (pendingX - startX) * sign);
      endTransition();
    };

    const onUp = (ev: PointerEvent) => {
      if (ev.pointerId !== pointerId) return;
      stop();
    };

    stopDrag.current = stop;
    window.addEventListener("pointermove", onMove);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("pointercancel", onUp);
  };

  useEffect(() => () => stopDrag.current?.(), []);

  const onDoubleClick = () => {
    commit(defaultRef.current);
  };

  /** Takes a width set elsewhere, without reporting it back as a commit. */
  const setPaneWidth = useCallback(
    (next: number) => {
      const value = clamp(next);
      preferredRef.current = next;
      apply(value);
      setWidth(value);
    },
    [clamp],
  );

  return {
    width,
    dragging,
    setPaneRef,
    setPaneWidth,
    onPointerDown,
    onDoubleClick,
  };
}
