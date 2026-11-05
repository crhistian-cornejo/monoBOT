import { describe, expect, it, vi } from "vitest";
import {
  allowsImageTool,
  imageTurnText,
  parseImageRequest,
  runImageTurn,
  type ImageTurnRunner,
} from "./generateImages";

describe("images.generate input", () => {
  it("reads a prompt with optional takes, aspect and source", () => {
    expect(parseImageRequest({ prompt: " a fox " })).toEqual({
      prompt: "a fox",
      count: 1,
    });
    expect(
      parseImageRequest({
        prompt: "a fox",
        count: 3,
        aspect: "16:9",
        source: "/tmp/a.png",
      }),
    ).toEqual({
      prompt: "a fox",
      count: 3,
      aspect: "16:9",
      source: "/tmp/a.png",
    });
    expect(() => parseImageRequest({ prompt: "" })).toThrow(/prompt/);
    expect(() => parseImageRequest({ prompt: "x", count: 9 })).toThrow(/count/);
    expect(() => parseImageRequest({ prompt: "x", aspect: "5:1" })).toThrow(
      /aspect/,
    );
    expect(() => parseImageRequest({ prompt: "x", source: "a.png" })).toThrow(
      /source/,
    );
  });

  it("asks Gemini for the takes and nothing else", () => {
    const text = imageTurnText({ prompt: "a fox", count: 2, aspect: "1:1" });
    expect(text).toContain("generate_image");
    expect(text).toContain("2 pictures");
    expect(text).toContain("Aspect ratio: 1:1.");
    expect(text).toContain("a fox");
  });

  it("lets only the picture tool through", () => {
    expect(allowsImageTool({ title: "generate_image" })).toBe(true);
    expect(allowsImageTool({ title: "Run shell", kind: "execute" })).toBe(
      false,
    );
  });
});

describe("runImageTurn", () => {
  const image = {
    path: "/a/1.jpg",
    name: "1.jpg",
    mimeType: "image/jpeg",
    size: 3,
  };

  it("returns the pictures and answers approvals on the throwaway session", async () => {
    const approve = vi.fn();
    const forget = vi.fn(async () => undefined);
    const runner: ImageTurnRunner = {
      approve,
      forget,
      send: async (input) => {
        input.onEvent({
          type: "approval.requested",
          requestId: 1,
          title: "Run shell",
          kind: "execute",
        });
        input.onEvent({
          type: "approval.requested",
          requestId: 2,
          title: "generate_image",
        });
        input.onEvent({ type: "image.generated", itemId: "i", ...image });
      },
    };
    const images = await runImageTurn(
      { prompt: "a fox", count: 1 },
      { cwd: "/tmp", model: "antigravity:m", runner, sessionId: "job" },
    );
    expect(images).toEqual([image]);
    expect(approve.mock.calls).toEqual([
      ["job", 1, false],
      ["job", 2, true],
    ]);
    expect(forget).toHaveBeenCalledWith("job");
  });

  it("fails with the provider's reason when nothing was made", async () => {
    const runner: ImageTurnRunner = {
      approve: () => {},
      forget: async () => {},
      send: async (input) =>
        input.onEvent({ type: "session.error", message: "Not signed in" }),
    };
    await expect(
      runImageTurn(
        { prompt: "x", count: 1 },
        { cwd: "/tmp", model: "m", runner },
      ),
    ).rejects.toThrow("Not signed in");
  });
});
