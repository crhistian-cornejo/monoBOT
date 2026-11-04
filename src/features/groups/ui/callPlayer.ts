import type { CallLanguage } from "../model/groupCall";
import { speakSentence } from "../model/voice";

/**
 * The call's speaker: sentences are synthesized as soon as they are queued
 * and played one after another in the order they came, so the next member
 * is ready by the time the one before finishes. `stop` silences everything
 * queued so far.
 */
export class CallPlayer {
  private readonly context: AudioContext;
  private chain: Promise<void> = Promise.resolve();
  private generation = 0;
  private current?: AudioBufferSourceNode;

  /** Made from the click that starts the call, so the page may play sound. */
  constructor(private readonly onError: (reason: unknown) => void) {
    this.context = new AudioContext();
    void this.context.resume().catch(() => undefined);
  }

  say(text: string, lang: CallLanguage, voice: number, onStart?: () => void) {
    const generation = this.generation;
    const audio = speakSentence(text, lang, voice).then((wav) =>
      this.context.decodeAudioData(wav),
    );
    // Rejections are reported when the sentence's turn to play comes.
    audio.catch(() => undefined);
    this.chain = this.chain.then(async () => {
      let buffer: AudioBuffer;
      try {
        buffer = await audio;
      } catch (reason) {
        if (generation === this.generation) this.onError(reason);
        return;
      }
      if (generation !== this.generation) return;
      onStart?.();
      await new Promise<void>((resolve) => {
        const node = this.context.createBufferSource();
        node.buffer = buffer;
        node.connect(this.context.destination);
        node.onended = () => {
          if (this.current === node) this.current = undefined;
          resolve();
        };
        this.current = node;
        node.start();
      });
    });
  }

  /** Settles once everything queued has played or been stopped. */
  drained(): Promise<void> {
    return this.chain;
  }

  stop() {
    this.generation++;
    try {
      this.current?.stop();
    } catch {
      // Already ended.
    }
    this.current = undefined;
  }

  close() {
    this.stop();
    void this.context.close().catch(() => undefined);
  }
}
