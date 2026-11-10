import { expect, it } from "vitest";

import { faceAs, faceMascot, isPersona, monoFace } from "@/features/mascots/faces";

it("draws the face the desktop sends, else the mascot", () => {
  expect(monoFace({ mascot: "ghost", face: "3d:ghost:coder" })).toBe("3d:ghost:coder");
  // A desktop from before personas.
  expect(monoFace({ mascot: "ghost" })).toBe("ghost");
  expect(monoFace({ mascot: "ghost", face: "" })).toBe("ghost");
});

it("tells a bot's persona from a pixel mascot", () => {
  expect(isPersona("3d:cat:writer")).toBe(true);
  expect(isPersona("cat")).toBe(false);
  expect(isPersona("3d:")).toBe(false);
  expect(isPersona("3d::writer")).toBe(false);
  expect(faceMascot("3d:cat:writer")).toBe("cat");
  expect(faceMascot("cat")).toBe("cat");
});

it("keeps a bot's outfit when the look picker tries another mascot", () => {
  expect(faceAs("3d:cat:writer", "robot")).toBe("3d:robot:writer");
  expect(faceAs("cat", "robot")).toBe("robot");
});
