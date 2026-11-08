import { collectAntigravityImages } from "../../../../platform/tauri/fs";
import type { HarnessEvent } from "../../core/types";

/** Gemini's image tool, or the subagent Antigravity hands pictures to. */
const IMAGE_TOOL = /generate_image|image[-_]generator/i;

/** Whether an ACP update shows the turn asking Gemini for a picture. */
export function updateMakesImage(params: unknown): boolean {
  try {
    return IMAGE_TOOL.test(JSON.stringify(params) ?? "");
  } catch {
    return false;
  }
}

type ImageEvent = Extract<
  HarnessEvent,
  { type: "image.generated"; path: string }
>;

/**
 * The pictures a turn made, once they are on disk. Gemini's image subagent
 * can still be writing when the turn ends, so this looks again every
 * `everyMs` until one shows up or `waitMs` passes.
 */
export async function awaitTurnImages(options: {
  sinceMs: number;
  waitMs?: number;
  everyMs?: number;
  stopped: () => boolean;
  collect?: (sinceMs: number) => Promise<
    { path: string; name: string; mimeType: string; size: number }[]
  >;
  sleep?: (ms: number) => Promise<void>;
}): Promise<ImageEvent[]> {
  const {
    sinceMs,
    waitMs = 120_000,
    everyMs = 2_000,
    stopped,
    collect = collectAntigravityImages,
    sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms)),
  } = options;
  const deadline = Date.now() + waitMs;
  for (;;) {
    if (stopped()) return [];
    const found = await collect(sinceMs).catch(() => []);
    if (found.length) {
      return found.map((image, index) => ({
        type: "image.generated",
        itemId: `antigravity-image-${sinceMs}-${index}`,
        path: image.path,
        name: image.name,
        mimeType: image.mimeType,
        size: image.size,
      }));
    }
    if (Date.now() + everyMs > deadline) return [];
    await sleep(everyMs);
  }
}
