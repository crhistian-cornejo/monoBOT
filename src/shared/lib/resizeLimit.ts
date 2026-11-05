/**
 * Feedback for a resize handle pushed past its limit: the handle glows in the
 * accent near the pointer, stronger the further the pointer goes, so a pane
 * that stops shrinking says why instead of just going still.
 */

export type ResizeAxis = "x" | "y";

/** How far past the limit, in px, the glow reaches full strength. */
const FULL_STRAIN_PX = 72;
/** The glow is already visible the moment the limit is hit. */
const FIRST_STRAIN = 0.35;
/** How far the glow leans toward the pointer at full strength, in px. */
const MAX_PULL_PX = 4;

/** 0 inside the limits, then rising from FIRST_STRAIN toward 1. */
export function limitStrain(overshoot: number): number {
  const past = Math.abs(overshoot);
  if (!Number.isFinite(past) || past < 1) return 0;
  const rise = 1 - Math.exp(-past / FULL_STRAIN_PX);
  return FIRST_STRAIN + (1 - FIRST_STRAIN) * rise;
}

/** How far the glow leans in the pointer's direction, in px. */
export function limitPull(overshoot: number): number {
  const strain = limitStrain(overshoot);
  if (!strain) return 0;
  return Math.round(Math.sign(overshoot) * strain * MAX_PULL_PX * 10) / 10;
}

/**
 * Paints the glow on `handle`. `overshoot` is how far, in screen px along
 * `axis`, the pointer is past where the handle stopped (signed: + is right or
 * down); `along` is the pointer's place along the handle, in px from its start.
 */
export function paintResizeLimit(
  handle: HTMLElement,
  axis: ResizeAxis,
  overshoot: number,
  along: number,
) {
  handle.dataset.resizeLimit = axis;
  handle.style.setProperty("--resize-strain", limitStrain(overshoot).toFixed(3));
  handle.style.setProperty("--resize-at", `${Math.round(along)}px`);
  handle.style.setProperty("--resize-pull", `${limitPull(overshoot)}px`);
}

/** Fades the glow out; the attribute stays so the fade can play. */
export function clearResizeLimit(handle: HTMLElement) {
  if (!handle.dataset.resizeLimit) return;
  handle.style.setProperty("--resize-strain", "0");
  handle.style.setProperty("--resize-pull", "0px");
}

/** The pointer's place along a handle on `axis`, in px from its start. */
export function alongHandle(
  handle: HTMLElement,
  axis: ResizeAxis,
  clientX: number,
  clientY: number,
): number {
  const rect = handle.getBoundingClientRect();
  return axis === "x" ? clientY - rect.top : clientX - rect.left;
}
