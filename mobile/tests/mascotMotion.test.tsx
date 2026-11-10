import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

const system = vi.hoisted(() => ({ reduceMotion: false }));
vi.mock("react-native-reanimated", () => ({
  useReducedMotion: () => system.reduceMotion,
  useFrameCallback: () => {},
  useSharedValue: (value: number) => ({ value }),
}));

import { useMascotMotion } from "@/features/mascots/mascotMotion";
import type { MascotStatus } from "@/features/mascots/PixelMascot";
import { AnimationsPaused } from "@/ui/AnimationsPaused";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
const moving = new Map<string, boolean>();
function Face({ id, status, still = false }: { id: string; status: MascotStatus; still?: boolean }) {
  moving.set(id, useMascotMotion(status, still));
  return null;
}
const faces = (
  <>
    <Face id="working" status="working" />
    <Face id="working in a list" status="working" still />
    <Face id="idle" status="idle" />
    <Face id="idle in a list" status="idle" still />
    <Face id="needs you" status="needs-you" />
  </>
);

beforeEach(() => {
  system.reduceMotion = false;
  moving.clear();
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
});

it("hops at work, breathes at rest outside lists, and holds still waiting on you", async () => {
  await act(async () => root.render(faces));
  expect(Object.fromEntries(moving)).toEqual({
    working: true,
    "working in a list": true,
    idle: true,
    "idle in a list": false,
    "needs you": false,
  });
});

it("holds every face still under Reduce Motion or paused animations", async () => {
  system.reduceMotion = true;
  await act(async () => root.render(faces));
  expect([...moving.values()].some(Boolean)).toBe(false);
  system.reduceMotion = false;
  moving.clear();
  await act(async () => root.render(<AnimationsPaused value>{faces}</AnimationsPaused>));
  expect([...moving.values()].some(Boolean)).toBe(false);
});
