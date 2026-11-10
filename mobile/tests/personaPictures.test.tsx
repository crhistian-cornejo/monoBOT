import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

type Call = { operation: string; face: string; color: string; pose: string; side: number; portrait: boolean; directory: string };
const native = vi.hoisted(() => ({
  calls: [] as Call[],
  finish: [] as (() => void)[],
}));
vi.mock("react-native", () => ({ PixelRatio: { get: () => 3 } }));
vi.mock("expo-file-system", () => {
  class Directory {
    uri: string;
    constructor(parent: { uri: string }, name: string) {
      this.uri = `${parent.uri}${name}/`;
    }
  }
  class File {
    uri: string;
    constructor(parent: { uri: string }, name: string) {
      this.uri = `${parent.uri}${name}`;
    }
  }
  return { Directory, File, Paths: { cache: { uri: "file:///Caches%20App/" } } };
});
vi.mock("@/features/connection/nativeLink", () => ({
  nativeCall: (call: Call) =>
    new Promise((resolve, reject) => {
      native.calls.push(call);
      native.finish.push(() =>
        call.color === "bad" ? reject(new Error("not a colour")) : resolve({ name: `${call.face}-${call.pose}.png` }),
      );
    }),
}));

import type { PersonaPictures, PersonaPose } from "@/features/mascots/personaPictures";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

let root: ReturnType<typeof createRoot>;
// Fresh each test: the pictures drawn and the queue live in the module.
let usePersonaPictures: typeof import("@/features/mascots/personaPictures").usePersonaPictures;
const seen = new Map<string, PersonaPictures>();
const ALL: PersonaPose[] = ["rest", "blink", "talk"];
function Face({ id, face, color = "#ff8800", size = 66, poses = ALL }: {
  id: string; face: string; color?: string; size?: number; poses?: PersonaPose[];
}) {
  seen.set(id, usePersonaPictures(face, color, size, poses));
  return null;
}
async function finishOne() {
  await act(async () => {
    native.finish.shift()?.();
  });
}

beforeEach(async () => {
  vi.resetModules();
  ({ usePersonaPictures } = await import("@/features/mascots/personaPictures"));
  native.calls.length = 0;
  native.finish.length = 0;
  seen.clear();
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
});

it("draws every face at rest first, two at a time", async () => {
  await act(async () =>
    root.render(
      <>
        <Face id="a" face="3d:cat:writer" />
        <Face id="b" face="3d:robot:coder" />
        <Face id="c" face="3d:ghost:designer" />
      </>,
    ),
  );
  expect(native.calls.map((call) => `${call.face}:${call.pose}`)).toEqual([
    "3d:cat:writer:rest",
    "3d:robot:coder:rest",
  ]);
  expect(seen.get("a")).toEqual({});
  await finishOne();
  expect(native.calls.at(-1)).toMatchObject({ face: "3d:ghost:designer", pose: "rest" });
  expect(seen.get("a")).toEqual({ rest: "file:///Caches%20App/personas/3d:cat:writer-rest.png" });
  while (native.finish.length) await finishOne();
  expect(native.calls).toHaveLength(9);
  expect(native.calls.slice(0, 3).every((call) => call.pose === "rest")).toBe(true);
  expect(Object.keys(seen.get("c") ?? {}).sort()).toEqual(["blink", "rest", "talk"]);
  // The native side takes a path, not a URI.
  expect(native.calls[0].directory).toBe("/Caches App/personas/");
});

it("draws a picture once for every avatar that shows it", async () => {
  await act(async () =>
    root.render(
      <>
        <Face id="a" face="3d:frog:writer" poses={["rest"]} />
        <Face id="b" face="3d:frog:writer" poses={["rest"]} />
      </>,
    ),
  );
  await finishOne();
  expect(native.calls).toHaveLength(1);
  expect(seen.get("a")?.rest).toBe(seen.get("b")?.rest);
  // Mounted again later, it shows at once.
  await act(async () => root.render(<Face id="c" face="3d:frog:writer" poses={["rest"]} />));
  expect(native.calls).toHaveLength(1);
  expect(seen.get("c")?.rest).toContain("3d:frog:writer-rest.png");
});

it("marks a picture that can't be drawn, so the avatar falls back", async () => {
  await act(async () => root.render(<Face id="a" face="3d:owl:writer" color="bad" poses={["rest"]} />));
  await finishOne();
  expect(seen.get("a")).toEqual({ rest: null });
});

it("sizes pictures in pixels and draws small avatars as portraits", async () => {
  await act(async () =>
    root.render(
      <>
        <Face id="small" face="3d:bee:writer" size={14} poses={["rest"]} />
        <Face id="big" face="3d:bee:coder" size={120} poses={["rest"]} />
      </>,
    ),
  );
  expect(native.calls.map(({ side, portrait }) => ({ side, portrait }))).toEqual([
    { side: 42, portrait: true },
    // The renderer's largest; the avatar scales it.
    { side: 192, portrait: false },
  ]);
  while (native.finish.length) await finishOne();
});

it("draws nothing when no pose is asked for", async () => {
  await act(async () => root.render(<Face id="a" face="ghost" poses={[]} />));
  expect(native.calls).toHaveLength(0);
  expect(seen.get("a")).toEqual({});
});
