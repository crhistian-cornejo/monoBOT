import type { ReactNode } from "react";
import { Image, StyleSheet, View } from "react-native";
import Animated, { useAnimatedStyle, useDerivedValue } from "react-native-reanimated";

import { useMascotClock } from "./mascotMotion";
import { usePersonaPictures, type PersonaPose } from "./personaPictures";

const RESTING: readonly PersonaPose[] = ["rest"];
const IDLE: readonly PersonaPose[] = ["rest", "blink"];
const WORKING: readonly PersonaPose[] = ["rest", "blink", "talk"];

/**
 * A personal bot's 3D persona, the desktop's (avatar.rs) on the phone. Its
 * pictures are drawn natively, once per pose; until the first lands, its
 * colour shows as a soft dot, and where none can be drawn `fallback` stands
 * in. While it moves, it breathes at rest and blinks on its own beat; at work
 * it hops and, three times a second, trades its rest for its mouth open and
 * hands up.
 */
export function PersonaMascot({
  face,
  color,
  working,
  size,
  moving,
  fallback,
}: {
  face: string;
  color: string;
  working: boolean;
  size: number;
  moving: boolean;
  fallback: ReactNode;
}) {
  const pictures = usePersonaPictures(face, color, size, moving ? (working ? WORKING : IDLE) : RESTING);
  const box = { width: size, height: size };
  if (pictures.rest === null) return fallback;
  if (!pictures.rest) {
    const dot = size * 0.56;
    return (
      <View style={[box, styles.centre]}>
        <View style={{ width: dot, height: dot, borderRadius: dot / 2, backgroundColor: color, opacity: 0.5 }} />
      </View>
    );
  }
  if (!moving) return <Image source={{ uri: pictures.rest }} style={box} fadeDuration={0} />;
  return (
    <MovingPersona
      rest={pictures.rest}
      blink={pictures.blink ?? undefined}
      talk={working ? (pictures.talk ?? undefined) : undefined}
      color={color}
      working={working}
      size={size}
    />
  );
}

function MovingPersona({
  rest,
  blink,
  talk,
  color,
  working,
  size,
}: {
  rest: string;
  blink?: string;
  talk?: string;
  color: string;
  working: boolean;
  size: number;
}) {
  const clock = useMascotClock();
  // Each bot blinks on a beat of its own, from its colour.
  const beat = ((parseInt(color.slice(1), 16) || 0) % 89) * 0.21;
  const canTalk = !!talk;
  const canBlink = !!blink;
  const pose = useDerivedValue(() => {
    const t = clock.get();
    if (canTalk && Math.floor(t * 3) % 2 === 1) return 2;
    if (canBlink && (t + beat) % 5.2 > 5.05) return 1;
    return 0;
  });
  // How far it is off the ground and how much taller it stretches, as parts
  // of its height: a hop while working, a breath at rest.
  const body = useAnimatedStyle(() => {
    const t = clock.get();
    const hop = Math.abs(Math.sin(t * 9));
    const breath = Math.sin(t * 2.5);
    const rise = working ? hop * 0.08 : breath * 0.012;
    const stretch = working ? hop * 0.06 : breath * 0.012;
    return {
      transform: [
        { translateY: -rise * size },
        { scaleX: 1 - stretch * 0.6 },
        { scaleY: 1 + stretch },
      ],
    };
  });
  const resting = useAnimatedStyle(() => ({ opacity: pose.get() === 0 ? 1 : 0 }));
  const blinking = useAnimatedStyle(() => ({ opacity: pose.get() === 1 ? 1 : 0 }));
  const talking = useAnimatedStyle(() => ({ opacity: pose.get() === 2 ? 1 : 0 }));
  return (
    <Animated.View style={[{ width: size, height: size }, styles.feet, body]}>
      <Animated.Image source={{ uri: rest }} style={[StyleSheet.absoluteFill, resting]} fadeDuration={0} />
      {blink ? (
        <Animated.Image source={{ uri: blink }} style={[StyleSheet.absoluteFill, blinking]} fadeDuration={0} />
      ) : null}
      {talk ? (
        <Animated.Image source={{ uri: talk }} style={[StyleSheet.absoluteFill, talking]} fadeDuration={0} />
      ) : null}
    </Animated.View>
  );
}

const styles = StyleSheet.create({
  centre: { alignItems: "center", justifyContent: "center" },
  // It stretches up from where it stands, as on the desktop.
  feet: { transformOrigin: "bottom" },
});
