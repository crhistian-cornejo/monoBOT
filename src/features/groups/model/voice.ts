import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { CallLanguage } from "./groupCall";

/** The call's voices (crates/voice through src-tauri/src/voice.rs):
 * Supertonic 3 speaks Spanish and English on this machine. */
export type VoiceStatus = {
  installed: boolean;
  installing: boolean;
  /** The model's download size in bytes. */
  bytes: number;
};

export const voiceStatus = () => invoke<VoiceStatus>("voice_status");
export const installVoice = () => invoke<void>("voice_install");
export const voiceSpeakers = () => invoke<number>("voice_speakers");

/** One sentence spoken in `speaker`'s voice, as WAV bytes. */
export const speakSentence = (
  text: string,
  lang: CallLanguage,
  speaker: number,
) => invoke<ArrayBuffer>("voice_speak", { text, lang, speaker });

export function onVoiceInstall(
  handler: (progress: { done: number; total: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ done: number; total: number }>("voice-install", (event) =>
    handler(event.payload),
  );
}
