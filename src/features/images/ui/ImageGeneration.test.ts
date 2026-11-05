// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { ImageJobMeta } from "../../sessions/model/session";

vi.mock("../../../platform/tauri/fs", () => ({
  readBinaryFile: vi.fn(
    async () =>
      new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
  ),
}));

const { frameRatio, ImageGeneration, jobState } =
  await import("./ImageGeneration");
const { sanitizeImageJob } = await import("../../sessions/data/sessionStore");

let container: HTMLDivElement;
let root: Root;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

const job: ImageJobMeta = {
  id: "job",
  prompt: "A fox in a scarf",
  count: 3,
  status: "generating",
  startedAt: Date.now(),
  images: [],
};

it("shows one breathing frame per take while Gemini works", () => {
  act(() => root.render(createElement(ImageGeneration, { job })));
  expect(container.querySelectorAll('[role="status"]')).toHaveLength(3);
  expect(container.textContent).toContain("Creating");
  expect(container.textContent).toContain("A fox in a scarf");
});

it("swaps the frames for the pictures, and says why when it fails", () => {
  const image = {
    path: "/a/fox.png",
    name: "fox.png",
    mimeType: "image/png",
    size: 8,
  };
  act(() =>
    root.render(
      createElement(ImageGeneration, {
        job: { ...job, status: "done", images: [image] },
      }),
    ),
  );
  expect(container.querySelectorAll('[role="status"]')).toHaveLength(0);
  expect(
    container.querySelector('button[aria-label="Open fox.png full screen"]'),
  ).not.toBeNull();
  act(() =>
    root.render(
      createElement(ImageGeneration, {
        job: { ...job, status: "failed", error: "Not signed in" },
      }),
    ),
  );
  expect(container.querySelector('[role="alert"]')?.textContent).toBe(
    "Not signed in",
  );
});

it("reads shapes and a run cut off by a quit", () => {
  expect(frameRatio("16:9")).toBe("16 / 9");
  expect(frameRatio(undefined)).toBe("1 / 1");
  expect(jobState(job, job.startedAt + 60_000)).toBe("generating");
  expect(jobState(job, job.startedAt + 11 * 60_000)).toBe("interrupted");
});

it("keeps a job through a save and drops a malformed one", () => {
  const image = {
    path: "/a/fox.png",
    name: "fox.png",
    mimeType: "image/png",
    size: 8,
  };
  expect(
    sanitizeImageJob({ ...job, aspect: "16:9", images: [image, { bad: 1 }] }),
  ).toEqual({
    ...job,
    aspect: "16:9",
    images: [image],
  });
  expect(sanitizeImageJob({ ...job, status: "weird" })).toBeUndefined();
});
