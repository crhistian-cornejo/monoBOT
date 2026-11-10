import { use } from "react";
import { useFrameCallback, useReducedMotion, useSharedValue } from "react-native-reanimated";

import { AnimationsPaused } from "@/ui/AnimationsPaused";

import type { MascotStatus } from "./PixelMascot";

/**
 * Whether a Mono's face moves, as on the desktop: at work it always hops; at
 * rest it breathes and blinks unless `still` (a list's rows, where many sit
 * together); waiting on you it holds still under its bulb. Nothing moves with
 * Reduce Motion on, or where animations are paused.
 */
export function useMascotMotion(status: MascotStatus, still: boolean) {
  const reduceMotion = useReducedMotion();
  const paused = use(AnimationsPaused);
  return !reduceMotion && !paused && (status === "working" || (!still && status !== "needs-you"));
}

/**
 * Seconds on the UI thread, set every frame while the calling face is
 * mounted. Every face reads the frame's own time, so their beats line up as
 * the desktop's do, and none of it touches the JS thread.
 */
export function useMascotClock() {
  const clock = useSharedValue(0);
  useFrameCallback(({ timestamp }) => {
    clock.set(timestamp / 1000);
  });
  return clock;
}
