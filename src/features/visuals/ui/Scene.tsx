import { useMemo, useRef, useState } from "react";

import { orbit, project } from "../model/scene";
import type { VisualNode } from "../model/visual";
import { mix, useWidth, visualColor } from "./charts";

/** A 3D scene as a wireframe the user can drag round. */
export function Scene({
  node,
}: {
  node: Extract<VisualNode, { kind: "scene" }>;
}) {
  const [box, available] = useWidth<HTMLDivElement>();
  const [angles, setAngles] = useState({ yaw: 0, pitch: 0 });
  const drag = useRef<{ x: number; y: number } | null>(null);
  const width = Math.min(node.width, available || node.width);
  const height = node.height * (width / node.width);
  const segments = useMemo(
    () =>
      project(
        node.objects,
        {
          ...node.camera,
          eye: orbit(node.camera.eye, node.camera.target, angles.yaw, angles.pitch),
        },
        width,
        height,
      ),
    [node.objects, node.camera, angles, width, height],
  );
  const far = segments[0]?.depth ?? 1;
  const near = segments[segments.length - 1]?.depth ?? 0;
  return (
    <div ref={box} className="min-w-0">
      <svg
        width={width}
        height={height}
        className="block cursor-grab touch-none active:cursor-grabbing"
        style={{ background: mix("var(--color-content)", 2) }}
        role="img"
        aria-label="3D scene; drag to turn it"
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = { x: event.clientX, y: event.clientY };
        }}
        onPointerMove={(event) => {
          const start = drag.current;
          if (!start) return;
          drag.current = { x: event.clientX, y: event.clientY };
          setAngles((current) => ({
            yaw: current.yaw - (event.clientX - start.x) * 0.01,
            pitch: current.pitch + (event.clientY - start.y) * 0.01,
          }));
        }}
        onPointerUp={() => (drag.current = null)}
        onDoubleClick={() => setAngles({ yaw: 0, pitch: 0 })}
      >
        {segments.map((segment, index) => (
          <line
            key={index}
            x1={segment.from[0]}
            y1={segment.from[1]}
            x2={segment.to[0]}
            y2={segment.to[1]}
            strokeWidth={1.25}
            strokeLinecap="round"
            // Nearer edges read stronger, so depth survives without fills.
            strokeOpacity={
              far > near ? 0.35 + 0.65 * ((far - segment.depth) / (far - near)) : 1
            }
            style={{ stroke: visualColor(node.objects[segment.object].color) }}
          />
        ))}
      </svg>
    </div>
  );
}
