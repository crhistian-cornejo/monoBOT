import { Directory, File, Paths } from "expo-file-system";
import { useEffect, useSyncExternalStore } from "react";
import { PixelRatio } from "react-native";

import { nativeCall } from "@/features/connection/nativeLink";

/** A persona at rest, with its eyes shut, and with its mouth open and hands
 *  up, the frame a working bot trades with its rest. */
export type PersonaPose = "rest" | "blink" | "talk";

/** A picture's file once drawn; missing while it is drawn, `null` if it
 *  can't be (a build without the renderer, a colour that isn't `#rrggbb`). */
export type PersonaPictures = Partial<Record<PersonaPose, string | null>>;

/** Below this many points a persona is a portrait, its head and shoulders,
 *  where it would be one on the desktop's Retina screens. */
const PORTRAIT_BELOW = 20;
/** The renderer's largest picture; a bigger avatar scales it, as on the desktop. */
const MAX_SIDE = 192;
/** Pictures drawn at once. Each keeps a core busy for tens of milliseconds. */
const AT_ONCE = 2;

type Job = {
  key: string;
  face: string;
  color: string;
  pose: PersonaPose;
  side: number;
  portrait: boolean;
};

let folder: Directory | null = null;
/** The native side draws into the cache: what iOS clears is drawn again. */
function personas() {
  folder ??= new Directory(Paths.cache, "personas");
  return folder;
}
/** A picture's file path, for native code that reads files, not URIs. */
export const picturePath = (uri: string) => decodeURIComponent(uri.replace(/^file:\/\//, ""));

const drawn = new Map<string, string | null>();
const queued = new Set<string>();
const waiting: Job[] = [];
let running = 0;
const listeners = new Set<() => void>();

async function draw({ key, ...job }: Job) {
  let uri: string | null = null;
  try {
    const { name } = await nativeCall<{ name: string }>({
      operation: "persona",
      ...job,
      directory: picturePath(personas().uri),
    });
    uri = new File(personas(), name).uri;
  } catch {
    uri = null;
  }
  running--;
  queued.delete(key);
  drawn.set(key, uri);
  listeners.forEach((notify) => notify());
  pump();
}

function pump() {
  while (running < AT_ONCE && waiting.length) {
    // Every face's resting picture first; blinks and talking after.
    const rest = waiting.findIndex((job) => job.pose === "rest");
    const [job] = waiting.splice(rest >= 0 ? rest : 0, 1);
    running++;
    void draw(job);
  }
}

let scheduled = false;
/** Pumped once the screen's avatars have all asked, so every resting picture
 *  is queued before the first one starts. */
function schedule() {
  if (scheduled) return;
  scheduled = true;
  queueMicrotask(() => {
    scheduled = false;
    pump();
  });
}

function subscribe(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

const keyOf = (face: string, color: string, pose: PersonaPose, side: number, portrait: boolean) =>
  `${face}|${color}|${pose}|${side}|${portrait ? "portrait" : "full"}`;

/**
 * The persona `face` in `color`, in each of `poses`, for an avatar `size`
 * points square. The phone draws them itself, as the desktop does, and keeps
 * them on disk: a picture drawn once loads at once after.
 */
export function usePersonaPictures(
  face: string,
  color: string,
  size: number,
  poses: readonly PersonaPose[],
): PersonaPictures {
  const side = Math.min(MAX_SIDE, Math.max(8, Math.round(size * PixelRatio.get())));
  const portrait = size < PORTRAIT_BELOW;
  const wanted = poses.join(",");
  useEffect(() => {
    if (!wanted) return;
    for (const pose of wanted.split(",") as PersonaPose[]) {
      const key = keyOf(face, color, pose, side, portrait);
      if (drawn.has(key) || queued.has(key)) continue;
      queued.add(key);
      waiting.push({ key, face, color, pose, side, portrait });
    }
    schedule();
  }, [face, color, side, portrait, wanted]);
  // A string, so an unchanged set of pictures reads as unchanged.
  const shown = useSyncExternalStore(subscribe, () =>
    poses
      .map((pose) => {
        const uri = drawn.get(keyOf(face, color, pose, side, portrait));
        return uri === undefined ? "" : uri === null ? "\0" : uri;
      })
      .join("\n"),
  );
  const pictures: PersonaPictures = {};
  shown.split("\n").forEach((uri, index) => {
    if (uri) pictures[poses[index]] = uri === "\0" ? null : uri;
  });
  return pictures;
}
