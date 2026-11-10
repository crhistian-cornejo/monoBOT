/**
 * A Mono's face as the desktop names it (`Mono::face`): a personal bot wears
 * a 3D persona, `3d:<mascot>:<role>`, and every other Mono its pixel mascot,
 * by the mascot's name. Desktops from before personas send no face.
 */
const PERSONA = "3d:";

export function monoFace(mono: { mascot: string; face?: string }): string {
  return mono.face || mono.mascot;
}

/** Whether a face is a bot's 3D persona rather than a pixel mascot. */
export function isPersona(face: string): boolean {
  return face.startsWith(PERSONA) && face.indexOf(":", PERSONA.length) > PERSONA.length;
}

/** The pixel mascot a face is made from. */
export function faceMascot(face: string): string {
  return isPersona(face) ? face.slice(PERSONA.length, face.indexOf(":", PERSONA.length)) : face;
}

/** The face it would wear as `mascot`, for the look picker: a bot keeps
 *  what its role dresses it in. */
export function faceAs(face: string, mascot: string): string {
  if (!isPersona(face)) return mascot;
  return `${PERSONA}${mascot}${face.slice(face.indexOf(":", PERSONA.length))}`;
}
