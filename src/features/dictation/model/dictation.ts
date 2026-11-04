import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Local dictation (crates/dictation through src-tauri/src/dictation.rs):
 * Parakeet v3 hears Spanish and English on this machine. */
export type DictationStatus = {
  installed: boolean;
  installing: boolean;
  /** The model's download size in bytes. */
  bytes: number;
};

export const dictationStatus = () => invoke<DictationStatus>("dictation_status");
export const installDictation = () => invoke<void>("dictation_install");
export const startDictation = () => invoke<void>("dictation_start");
export const stopDictation = () => invoke<string>("dictation_stop");
export const cancelDictation = () => invoke<void>("dictation_cancel");

export function onDictationLevel(handler: (level: number) => void): Promise<UnlistenFn> {
  return listen<number>("dictation-level", (event) => handler(event.payload));
}

export function onDictationPartial(handler: (text: string) => void): Promise<UnlistenFn> {
  return listen<string>("dictation-partial", (event) => handler(event.payload));
}

/** The voice detector heard the user start (true) or stop (false) talking. */
export function onDictationSpeech(handler: (speaking: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("dictation-speech", (event) => handler(event.payload));
}

export function onDictationInstall(
  handler: (progress: { done: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ done: number; total: number }>("dictation-install", (event) =>
    handler(event.payload),
  );
}

/** Joins dictated text into a draft at the cursor, with one space between. */
export function insertDictated(
  value: string,
  start: number,
  end: number,
  text: string,
): { value: string; cursor: number } {
  const spoken = text.trim();
  if (!spoken) return { value, cursor: end };
  const before = value.slice(0, start);
  const after = value.slice(end);
  const lead = before && !/\s$/.test(before) ? " " : "";
  const trail = after && !/^\s/.test(after) ? " " : "";
  const inserted = `${lead}${spoken}${trail}`;
  return {
    value: `${before}${inserted}${after}`,
    cursor: before.length + lead.length + spoken.length,
  };
}

export function formatModelSize(bytes: number): string {
  return `${Math.round(bytes / 1_000_000)} MB`;
}
