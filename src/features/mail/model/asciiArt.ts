/** The glyphs of the art, from the emptiest to the densest. */
export const GLYPHS = " .:-=+*#%@";

const CURVE = 1.5;

export interface AsciiCell { glyph: string; light: number }

/** The part of a picture that covers a panel: its centre, as `object-cover` keeps it. */
export function coverCrop(width: number, height: number, panelWidth: number, panelHeight: number): { x: number; y: number; width: number; height: number } {
  const scale = Math.max(panelWidth / width, panelHeight / height);
  const cropWidth = Math.round(panelWidth / scale), cropHeight = Math.round(panelHeight / scale);
  return { x: Math.round((width - cropWidth) / 2), y: Math.round((height - cropHeight) / 2), width: cropWidth, height: cropHeight };
}

/** A picture as glyphs: `pixels` are its RGBA bytes at one pixel per cell. Its tones are stretched between its darkest and
    lightest few, so a picture of middling tones still goes from empty to dense, and a stray bright point does not flatten it. */
export function asciiCells(pixels: Uint8ClampedArray, cols: number, rows: number): AsciiCell[] {
  const count = cols * rows;
  const tones = new Float32Array(count);
  for (let at = 0; at < count; at++) {
    const from = at * 4;
    tones[at] = (0.2126 * pixels[from]! + 0.7152 * pixels[from + 1]! + 0.0722 * pixels[from + 2]!) / 255;
  }
  const sorted = Float32Array.from(tones).sort();
  const low = sorted[Math.floor(count * 0.02)] ?? 0;
  const high = sorted[Math.min(count - 1, Math.floor(count * 0.98))] ?? 1;
  const range = high - low || 1;
  const cells: AsciiCell[] = [];
  for (let at = 0; at < count; at++) {
    // (The curve keeps the middle tones sparse: the eye goes to what is really bright, not to a wall of glyphs.)
    const light = Math.min(1, Math.max(0, (tones[at]! - low) / range)) ** CURVE;
    cells.push({ glyph: GLYPHS[Math.min(GLYPHS.length - 1, Math.floor(light * GLYPHS.length))]!, light });
  }
  return cells;
}
