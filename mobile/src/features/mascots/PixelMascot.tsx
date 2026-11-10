import { StyleSheet, View, type ViewStyle } from "react-native";
import Animated, { useAnimatedStyle, useDerivedValue, type AnimatedStyle } from "react-native-reanimated";

import { withAlpha, themedStyles } from "@/ui/theme";

import { faceMascot, isPersona } from "./faces";
import { useMascotClock, useMascotMotion } from "./mascotMotion";
import { PersonaMascot } from "./PersonaMascot";
import {
  BULB,
  PIXEL_GRID,
  PIXEL_MASCOT_NAMES,
  pixelLayers,
  pixelOffset,
  type PixelLayers,
  type PixelRect,
} from "./pixelMascots";

/** What a mascot's agent is doing, as on desktop. */
export type MascotStatus = "idle" | "working" | "needs-you";

function Rects({ rects, unit }: { rects: readonly PixelRect[]; unit: number }) {
  return (
    <>
      {rects.map(({ x, y, w, h, fill }, i) => (
        <View
          key={i}
          style={{
            position: "absolute",
            left: x * unit,
            top: y * unit,
            width: w * unit,
            height: h * unit,
            backgroundColor: fill,
          }}
        />
      ))}
    </>
  );
}

/**
 * A Mono's face, the native take on the desktop's avatar. `name` is a pixel
 * mascot's name or a bot's 3D persona (`monoFace`). Pixel mascots are drawn
 * with Views from the same rects, so there is no SVG dependency.
 *
 * As on the desktop, it hops while working, trading frames three times a
 * second; at rest it bobs and blinks, unless `still`; when the Mono needs you
 * it holds still under a bulb. The motion runs on the UI thread, and not at
 * all with Reduce Motion on.
 */
export function PixelMascot({
  name,
  color,
  status = "idle",
  size = 22,
  centered = false,
  still = false,
}: {
  name: string;
  color: string;
  status?: MascotStatus;
  size?: number;
  /** Centres the figure itself in its square, as a picker's tiles want. */
  centered?: boolean;
  /** Holds still at rest, as a list's rows do; it still hops at work. */
  still?: boolean;
}) {
  const moving = useMascotMotion(status, still);
  const unit = size / PIXEL_GRID;
  const working = status === "working";
  const pixels = (mascot: string) => {
    const offset = centered ? pixelOffset(mascot) : { x: 0, y: 0 };
    const frame = { left: offset.x * unit, top: offset.y * unit, width: size, height: size };
    return moving ? (
      <MovingPixels name={mascot} color={color} unit={unit} frame={frame} working={working} />
    ) : (
      <View style={[layout.frame, frame]}>
        <Figure layers={pixelLayers(mascot, color)} unit={unit} />
      </View>
    );
  };

  return (
    <View style={{ width: size, height: size }} accessibilityElementsHidden importantForAccessibility="no-hide-descendants">
      {isPersona(name) ? (
        <PersonaMascot
          face={name}
          color={color}
          working={working}
          size={size}
          moving={moving}
          fallback={pixels(faceMascot(name))}
        />
      ) : (
        pixels(name)
      )}
      {status === "needs-you" ? (
        <View style={{ position: "absolute", left: 12.5 * unit, top: -2.5 * unit }}>
          <Rects rects={BULB} unit={unit} />
        </View>
      ) : null}
    </View>
  );
}

/** One frame of a pixel mascot, its eyes open or, under a blink, shut. */
function Figure({
  layers,
  unit,
  open,
  shut,
}: {
  layers: PixelLayers;
  unit: number;
  open?: AnimatedStyle<ViewStyle>;
  shut?: AnimatedStyle<ViewStyle>;
}) {
  return (
    <>
      <Rects rects={layers.body} unit={unit} />
      {open ? (
        <Animated.View style={[StyleSheet.absoluteFill, open]}>
          <Rects rects={layers.open} unit={unit} />
        </Animated.View>
      ) : (
        <Rects rects={layers.open} unit={unit} />
      )}
      {shut ? (
        <Animated.View style={[StyleSheet.absoluteFill, shut]}>
          <Rects rects={layers.closed} unit={unit} />
        </Animated.View>
      ) : null}
    </>
  );
}

/** Where each mascot blinks in the shared beat, as the desktop offsets them. */
function blinkOffset(name: string) {
  const index = PIXEL_MASCOT_NAMES.indexOf(name);
  return (index >= 0 ? index : PIXEL_MASCOT_NAMES.indexOf("ghost")) * 0.37;
}

function MovingPixels({
  name,
  color,
  unit,
  frame,
  working,
}: {
  name: string;
  color: string;
  unit: number;
  frame: ViewStyle;
  working: boolean;
}) {
  const clock = useMascotClock();
  const offset = blinkOffset(name);
  // In grid units: a hop off the ground at work, a gentle bob at rest.
  const hop = useAnimatedStyle(() => {
    const t = clock.get();
    const lift = working ? -Math.abs(Math.sin(t * 9)) * 0.9 : Math.sin(t * 2.5) * 0.25;
    return { transform: [{ translateY: lift * unit }] };
  });
  const talking = useDerivedValue(() => working && Math.floor(clock.get() * 3) % 2 === 1);
  const blinking = useDerivedValue(() => (clock.get() + offset) % 5.2 > 5.05);
  const rest = useAnimatedStyle(() => ({ opacity: talking.get() ? 0 : 1 }));
  const talk = useAnimatedStyle(() => ({ opacity: talking.get() ? 1 : 0 }));
  const open = useAnimatedStyle(() => ({ opacity: blinking.get() ? 0 : 1 }));
  const shut = useAnimatedStyle(() => ({ opacity: blinking.get() ? 1 : 0 }));
  return (
    <Animated.View style={[layout.frame, frame, hop]}>
      <Animated.View style={[StyleSheet.absoluteFill, rest]}>
        <Figure layers={pixelLayers(name, color)} unit={unit} open={open} shut={shut} />
      </Animated.View>
      {working ? (
        <Animated.View style={[StyleSheet.absoluteFill, talk]}>
          <Figure layers={pixelLayers(name, color, true)} unit={unit} open={open} shut={shut} />
        </Animated.View>
      ) : null}
    </Animated.View>
  );
}

/** Mascot plus the desktop's status circle in the top-right corner. */
export function MonoMascot({
  name,
  color,
  status = "idle",
  size = 22,
  still = false,
}: {
  name: string;
  color: string;
  status?: MascotStatus;
  size?: number;
  still?: boolean;
}) {
  const styles = useStyles();
  return (
    <View style={{ width: size, height: size }}>
      <PixelMascot name={name} color={color} status={status} size={size} still={still} />
      <View
        style={[
          styles.dot,
          status === "working"
            ? { backgroundColor: color }
            : status === "needs-you"
              ? styles.dotNeedsYou
              : styles.dotIdle,
        ]}
      />
    </View>
  );
}

const layout = StyleSheet.create({
  frame: { position: "absolute" },
});

const useStyles = themedStyles((colors) => ({
  dot: {
    position: "absolute",
    top: -2,
    right: -2,
    width: 7,
    height: 7,
    borderRadius: 4,
    borderWidth: 1,
    borderColor: colors.drawer,
  },
  dotNeedsYou: {
    backgroundColor: colors.accent,
  },
  dotIdle: {
    backgroundColor: withAlpha(colors.content, 0.3),
  },
}));
