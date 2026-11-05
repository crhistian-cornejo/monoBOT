import { describe, expect, it } from "vitest";

import { edges, orbit, project, transform } from "./scene";
import type { SceneObject } from "./visual";

const box: SceneObject = {
  geometry: { kind: "box" },
  position: [0, 0, 0],
  rotation: [0, 0, 0],
  scale: [1, 1, 1],
  color: "accent",
};

describe("scene", () => {
  it("draws a box's twelve edges and a mesh's edges once", () => {
    expect(edges({ kind: "box" })).toHaveLength(12);
    expect(
      edges({
        kind: "mesh",
        vertices: [
          [0, 0, 0],
          [1, 0, 0],
          [0, 1, 0],
          [1, 1, 0],
        ],
        indices: [
          [0, 1, 2],
          [1, 3, 2],
        ],
      }),
    ).toHaveLength(5);
  });

  it("scales, turns and moves a point", () => {
    const point = transform(
      { ...box, scale: [2, 2, 2], rotation: [0, Math.PI / 2, 0], position: [0, 1, 0] },
      [1, 0, 0],
    );
    expect(point.map((v) => Math.round(v * 1e6) / 1e6)).toEqual([0, 1, -2]);
  });

  it("orbits at the same distance and keeps the target centred", () => {
    const eye = orbit([0, 0, 5], [0, 0, 0], Math.PI / 2, 0);
    expect(Math.hypot(...eye)).toBeCloseTo(5);
    expect(eye[0]).toBeCloseTo(5);
    const segments = project(
      [{ ...box, geometry: { kind: "line", points: [[0, 0, 0], [0, 0, 0]], width: 1 } }],
      { eye: [0, 0, 5], target: [0, 0, 0], fov: 45 },
      200,
      100,
    );
    expect(segments[0].from).toEqual([100, 50]);
  });

  it("drops what sits behind the camera", () => {
    expect(
      project([{ ...box, position: [0, 0, 10] }], { eye: [0, 0, 5], target: [0, 0, 0], fov: 45 }, 100, 100),
    ).toEqual([]);
  });
});
