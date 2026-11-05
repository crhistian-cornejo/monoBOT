// A visual's 3D scene as wireframe segments on screen: each object's edges,
// transformed, seen from the camera, orbited by the user's drag.
import type { SceneObject } from "./visual";

export type Vec3 = [number, number, number];

const sub = (a: Vec3, b: Vec3): Vec3 => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a: Vec3, b: Vec3) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a: Vec3, b: Vec3): Vec3 => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
];
const unit = (a: Vec3): Vec3 => {
  const length = Math.hypot(...a) || 1;
  return [a[0] / length, a[1] / length, a[2] / length];
};

const ring = (y: number, radius: number, steps = 24): Vec3[] =>
  Array.from({ length: steps + 1 }, (_, i) => {
    const a = (i / steps) * Math.PI * 2;
    return [Math.cos(a) * radius, y, Math.sin(a) * radius];
  });

const pairs = (points: Vec3[]): [Vec3, Vec3][] =>
  points.slice(1).map((point, i) => [points[i], point]);

/** An object's edges in its own unit space. */
export function edges(geometry: SceneObject["geometry"]): [Vec3, Vec3][] {
  switch (geometry.kind) {
    case "box": {
      const c: Vec3[] = [];
      for (const x of [-0.5, 0.5])
        for (const y of [-0.5, 0.5]) for (const z of [-0.5, 0.5]) c.push([x, y, z]);
      const out: [Vec3, Vec3][] = [];
      for (let i = 0; i < 8; i++)
        for (let j = i + 1; j < 8; j++) {
          const differ = c[i].filter((v, k) => v !== c[j][k]).length;
          if (differ === 1) out.push([c[i], c[j]]);
        }
      return out;
    }
    case "plane":
      return pairs([
        [-0.5, 0, -0.5],
        [0.5, 0, -0.5],
        [0.5, 0, 0.5],
        [-0.5, 0, 0.5],
        [-0.5, 0, -0.5],
      ]);
    case "sphere": {
      const out = [-0.35, 0, 0.35].flatMap((y) =>
        pairs(ring(y, Math.sqrt(0.25 - y * y))),
      );
      for (let m = 0; m < 4; m++) {
        const a = (m / 4) * Math.PI;
        out.push(
          ...pairs(
            Array.from({ length: 25 }, (_, i) => {
              const t = (i / 24) * Math.PI * 2;
              return [
                Math.cos(a) * Math.sin(t) * 0.5,
                Math.cos(t) * 0.5,
                Math.sin(a) * Math.sin(t) * 0.5,
              ] as Vec3;
            }),
          ),
        );
      }
      return out;
    }
    case "cylinder":
    case "cone": {
      const cone = geometry.kind === "cone";
      const out = [...pairs(ring(-0.5, 0.5))];
      if (!cone) out.push(...pairs(ring(0.5, 0.5)));
      for (let i = 0; i < 4; i++) {
        const a = (i / 4) * Math.PI * 2;
        const foot: Vec3 = [Math.cos(a) * 0.5, -0.5, Math.sin(a) * 0.5];
        out.push([foot, cone ? [0, 0.5, 0] : [foot[0], 0.5, foot[2]]]);
      }
      return out;
    }
    case "mesh": {
      const seen = new Set<string>();
      const out: [Vec3, Vec3][] = [];
      for (const triangle of geometry.indices)
        for (let k = 0; k < 3; k++) {
          const a = triangle[k];
          const b = triangle[(k + 1) % 3];
          const key = a < b ? `${a}:${b}` : `${b}:${a}`;
          const from = geometry.vertices[a];
          const to = geometry.vertices[b];
          if (seen.has(key) || !from || !to) continue;
          seen.add(key);
          out.push([from, to]);
        }
      return out;
    }
    case "line":
      return pairs(geometry.points);
  }
}

/** Scale, then rotate about x, y and z, then move. */
export function transform(object: SceneObject, point: Vec3): Vec3 {
  let [x, y, z] = [
    point[0] * object.scale[0],
    point[1] * object.scale[1],
    point[2] * object.scale[2],
  ];
  const [rx, ry, rz] = object.rotation;
  [y, z] = [y * Math.cos(rx) - z * Math.sin(rx), y * Math.sin(rx) + z * Math.cos(rx)];
  [x, z] = [x * Math.cos(ry) + z * Math.sin(ry), -x * Math.sin(ry) + z * Math.cos(ry)];
  [x, y] = [x * Math.cos(rz) - y * Math.sin(rz), x * Math.sin(rz) + y * Math.cos(rz)];
  return [x + object.position[0], y + object.position[1], z + object.position[2]];
}

/** The eye after the user orbits it `yaw` and `pitch` radians round the target. */
export function orbit(eye: Vec3, target: Vec3, yaw: number, pitch: number): Vec3 {
  const offset = sub(eye, target);
  const distance = Math.hypot(...offset) || 1;
  const azimuth = Math.atan2(offset[0], offset[2]) + yaw;
  const elevation = Math.min(
    Math.max(Math.asin(offset[1] / distance) + pitch, -1.5),
    1.5,
  );
  return [
    target[0] + distance * Math.cos(elevation) * Math.sin(azimuth),
    target[1] + distance * Math.sin(elevation),
    target[2] + distance * Math.cos(elevation) * Math.cos(azimuth),
  ];
}

export type Segment = {
  from: [number, number];
  to: [number, number];
  depth: number;
  object: number;
};

/** Every object's edges on a `width` × `height` screen, farthest first. */
export function project(
  objects: readonly SceneObject[],
  camera: { eye: Vec3; target: Vec3; fov: number },
  width: number,
  height: number,
): Segment[] {
  const forward = unit(sub(camera.target, camera.eye));
  let right = cross(forward, [0, 1, 0]);
  if (Math.hypot(...right) < 1e-6) right = [1, 0, 0];
  right = unit(right);
  const up = cross(right, forward);
  const focal =
    height / 2 / Math.tan(((Math.min(Math.max(camera.fov, 10), 120) * Math.PI) / 180) / 2);
  const screen = (point: Vec3): [number, number, number] | null => {
    const relative = sub(point, camera.eye);
    const z = dot(relative, forward);
    if (z < 0.05) return null;
    return [
      width / 2 + (dot(relative, right) / z) * focal,
      height / 2 - (dot(relative, up) / z) * focal,
      z,
    ];
  };
  const segments: Segment[] = [];
  objects.forEach((object, index) => {
    for (const [a, b] of edges(object.geometry)) {
      const from = screen(transform(object, a));
      const to = screen(transform(object, b));
      if (!from || !to) continue;
      segments.push({
        from: [from[0], from[1]],
        to: [to[0], to[1]],
        depth: (from[2] + to[2]) / 2,
        object: index,
      });
    }
  });
  return segments.sort((a, b) => b.depth - a.depth);
}
