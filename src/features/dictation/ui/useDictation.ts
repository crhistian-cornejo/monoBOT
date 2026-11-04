import { useCallback, useEffect, useRef, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import {
  cancelDictation,
  dictationStatus,
  installDictation,
  onDictationInstall,
  onDictationLevel,
  onDictationPartial,
  startDictation,
  stopDictation,
} from "../model/dictation";

export type DictationPhase =
  | "idle"
  | "needs-model"
  | "installing"
  | "starting"
  | "listening"
  | "finishing";

const BARS = 40;

export type Dictation = {
  phase: DictationPhase;
  /** Recent loudness, oldest first, for the waveform. */
  levels: number[];
  /** What has been heard so far, before it lands in the draft. */
  partial: string;
  /** Download progress, 0 to 1, while the model installs. */
  progress: number;
  modelBytes: number;
  error?: string;
  toggle: () => void;
  install: () => void;
  cancel: () => void;
  dismiss: () => void;
};

/**
 * One composer's dictation. The text lands in the draft when listening
 * stops; Escape drops it instead.
 */
export function useDictation(onText: (text: string) => void): Dictation {
  const [phase, setPhase] = useState<DictationPhase>("idle");
  const [levels, setLevels] = useState<number[]>(() => Array(BARS).fill(0));
  const [partial, setPartial] = useState("");
  const [progress, setProgress] = useState(0);
  const [modelBytes, setModelBytes] = useState(0);
  const [error, setError] = useState<string>();
  const listeners = useRef<UnlistenFn[]>([]);
  const onTextRef = useRef(onText);
  onTextRef.current = onText;
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  const unlisten = () => {
    for (const stop of listeners.current) stop();
    listeners.current = [];
  };
  useEffect(() => unlisten, []);

  const fail = (reason: unknown) => {
    unlisten();
    setError(String(reason));
    setPhase("idle");
  };

  const start = useCallback(async () => {
    setError(undefined);
    setPartial("");
    setLevels(Array(BARS).fill(0));
    setPhase("starting");
    try {
      listeners.current.push(
        await onDictationLevel((level) =>
          setLevels((current) => [...current.slice(1), level]),
        ),
        await onDictationPartial(setPartial),
      );
      await startDictation();
      setPhase("listening");
    } catch (reason) {
      fail(reason);
    }
  }, []);

  const finish = useCallback(async () => {
    setPhase("finishing");
    try {
      const text = await stopDictation();
      unlisten();
      setPhase("idle");
      setPartial("");
      if (text.trim()) onTextRef.current(text);
    } catch (reason) {
      fail(reason);
    }
  }, []);

  const toggle = useCallback(() => {
    const current = phaseRef.current;
    if (current === "listening") return void finish();
    if (current !== "idle" && current !== "needs-model") return;
    void dictationStatus()
      .then((status) => {
        setModelBytes(status.bytes);
        if (status.installing) return setPhase("installing");
        if (!status.installed) return setPhase("needs-model");
        return start();
      })
      .catch(fail);
  }, [finish, start]);

  const install = useCallback(() => {
    setError(undefined);
    setProgress(0);
    setPhase("installing");
    void (async () => {
      const stop = await onDictationInstall(({ done, total }) =>
        setProgress(total ? done / total : 0),
      );
      try {
        await installDictation();
        stop();
        await start();
      } catch (reason) {
        stop();
        fail(reason);
      }
    })();
  }, [start]);

  const cancel = useCallback(() => {
    if (phaseRef.current !== "listening" && phaseRef.current !== "starting")
      return;
    void cancelDictation().finally(() => {
      unlisten();
      setPartial("");
      setPhase("idle");
    });
  }, []);

  const dismiss = useCallback(() => {
    setError(undefined);
    if (phaseRef.current === "needs-model") setPhase("idle");
  }, []);

  // Escape drops a dictation in progress, before the composer sees the key.
  useEffect(() => {
    if (phase !== "listening" && phase !== "starting") return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      cancel();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [phase, cancel]);

  return {
    phase,
    levels,
    partial,
    progress,
    modelBytes,
    error,
    toggle,
    install,
    cancel,
    dismiss,
  };
}
