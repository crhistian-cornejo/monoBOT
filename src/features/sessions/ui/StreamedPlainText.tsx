import { useEffect, useLayoutEffect, useRef, type CSSProperties } from "react";
import { usePacedReveal } from "../../settings/model/displayPrefs";
import { PACED_STAGGER_CAP_MS } from "../model/wordMotion";
import {
  pacedRevealPreferred,
  staggerNewWords,
  useCadenceFade,
  usePacedText,
  useStreamedText,
  useWordFading,
  WORD_FADE_MS,
} from "./wordFade";

/**
 * Plain text an agent is writing, shown the way a chat reply streams: paced,
 * each word coming into focus in the accent colour. For text that is not
 * Markdown, such as an email, where `AgentMarkdown` would read `*` or `1.` as
 * formatting. `onSettled` fires once the last word has finished its fade.
 */
export function StreamedPlainText({
  text,
  streaming,
  className = "",
  onSettled,
  onRevealing,
}: {
  text: string;
  streaming: boolean;
  className?: string;
  onSettled?: () => void;
  /** Whether the words on screen are still catching up with `text`. */
  onRevealing?: (revealing: boolean) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  const pacingEnabled = usePacedReveal();
  const pacedReveal = pacingEnabled && pacedRevealPreferred();
  // Both hooks keep their identity when the preference changes mid-reply.
  const smooth = usePacedText(text, streaming, true, pacedReveal);
  const immediate = useStreamedText(text, streaming, true, !pacedReveal);
  const shown = pacedReveal ? smooth : immediate;
  const writing = streaming || shown.revealing;
  const cadence = useCadenceFade(shown.text);
  const fadeMs = pacedReveal ? WORD_FADE_MS : cadence.durationMs;
  const fading = useWordFading(writing, fadeMs);

  // A burst let out in one paint still reads a word at a time.
  useLayoutEffect(() => {
    if (!fading || !box.current) return;
    staggerNewWords(box.current, pacedReveal ? PACED_STAGGER_CAP_MS : Math.round(fadeMs / 2));
    box.current.scrollTop = box.current.scrollHeight;
  });

  const revealingChanged = useRef(onRevealing);
  revealingChanged.current = onRevealing;
  useEffect(() => {
    revealingChanged.current?.(shown.revealing);
  }, [shown.revealing]);

  const settled = useRef(onSettled);
  settled.current = onSettled;
  useEffect(() => {
    if (!writing && !fading) settled.current?.();
  }, [writing, fading]);

  // Words and the spaces between them alternate, so each word keeps its place
  // (and its span) as the text grows: a word on screen never fades twice.
  const parts = shown.text.split(/(\s+)/);
  const style = pacedReveal
    ? undefined
    : ({
        "--word-fade-ms": `${fadeMs}ms`,
        "--word-fade-ease": cadence.easing,
      } as CSSProperties);

  return (
    <div
      ref={box}
      aria-live="polite"
      style={style}
      className={`stream-words whitespace-pre-wrap break-words ${fading ? "word-fading" : ""} ${className}`}
    >
      {parts.map((part, index) =>
        fading && part && !/^\s/.test(part) ? (
          <span key={index} data-word-fade="">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </div>
  );
}
