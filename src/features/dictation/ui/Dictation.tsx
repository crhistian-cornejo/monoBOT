import { Mic, Square } from "../../../shared/ui/icons";
import { formatModelSize } from "../model/dictation";
import type { Dictation } from "./useDictation";

/** The composer's mic: starts listening, and stops to put the text in. */
export function DictationButton({ dictation }: { dictation: Dictation }) {
  const { phase } = dictation;
  const listening = phase === "listening";
  const busy = phase === "starting" || phase === "finishing" || phase === "installing";
  const label = listening
    ? "Stop dictation"
    : phase === "installing"
      ? "Downloading the speech model"
      : "Dictate";
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      aria-pressed={listening}
      disabled={busy}
      onClick={dictation.toggle}
      data-dictation={phase}
      className={`grid size-6.5 shrink-0 place-items-center ${
        listening
          ? "bg-accent text-white"
          : "text-content/60 hover:bg-content/8 hover:text-content"
      } disabled:opacity-50`}
    >
      {listening ? (
        <Square className="size-2.5 fill-current" strokeWidth={0} />
      ) : (
        <Mic className="size-3.5" strokeWidth={1.75} />
      )}
    </button>
  );
}

/** Live bars of how loud the microphone is, newest on the right. */
export function DictationWave({ levels }: { levels: readonly number[] }) {
  return (
    <div aria-hidden className="flex h-5 items-center gap-[2px]" data-dictation-wave>
      {levels.map((level, index) => (
        <span
          key={index}
          className="w-[2px] bg-accent transition-[height] duration-75"
          style={{ height: `${Math.max(2, Math.round(level * 20))}px` }}
        />
      ))}
    </div>
  );
}

/**
 * What the composer shows above its toolbar while dictation is active: the
 * waveform and the words heard so far, the model download, or an error.
 */
export function DictationBar({ dictation }: { dictation: Dictation }) {
  const { phase, error } = dictation;
  if (error)
    return (
      <div className="flex items-center gap-2 border-t border-stroke px-3 py-1.5 text-xs">
        <span className="min-w-0 flex-1 truncate text-red-400">{error}</span>
        <button
          type="button"
          onClick={dictation.dismiss}
          className="text-content/55 hover:text-content"
        >
          Dismiss
        </button>
      </div>
    );
  if (phase === "needs-model")
    return (
      <div className="flex items-center gap-2 border-t border-stroke px-3 py-1.5 text-xs">
        <span className="min-w-0 flex-1 text-content/70">
          Dictation runs on this computer. Download the speech model (
          {formatModelSize(dictation.modelBytes)}, Spanish and English) once to
          start.
        </span>
        <button
          type="button"
          onClick={dictation.dismiss}
          className="text-content/55 hover:text-content"
        >
          Not now
        </button>
        <button
          type="button"
          onClick={dictation.install}
          className="bg-accent px-2 py-0.5 font-medium text-white"
        >
          Download
        </button>
      </div>
    );
  if (phase === "installing")
    return (
      <div className="flex items-center gap-3 border-t border-stroke px-3 py-1.5 text-xs text-content/70">
        <span className="shrink-0">Downloading the speech model…</span>
        <span className="relative h-1 min-w-0 flex-1 bg-content/10">
          <span
            className="absolute inset-y-0 left-0 bg-accent"
            style={{ width: `${Math.round(dictation.progress * 100)}%` }}
          />
        </span>
        <span className="shrink-0 tabular-nums">
          {Math.round(dictation.progress * 100)}%
        </span>
      </div>
    );
  if (phase !== "listening" && phase !== "starting" && phase !== "finishing")
    return null;
  return (
    <div
      className="flex items-center gap-3 border-t border-stroke px-3 py-1.5"
      data-dictation-bar
    >
      <DictationWave levels={dictation.levels} />
      <span className="min-w-0 flex-1 truncate text-xs text-content/50">
        {phase === "starting"
          ? "Starting the microphone…"
          : phase === "finishing"
            ? dictation.partial || "Finishing…"
            : dictation.partial || "Listening…"}
      </span>
      <span className="shrink-0 font-mono text-[11px] uppercase tracking-wide text-content/40">
        Esc to cancel
      </span>
    </div>
  );
}
