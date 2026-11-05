import { useEffect, useRef, useState } from "react";
import picture from "../../../assets/mail/mail-art.webp";
import { asciiCells, coverCrop } from "../model/asciiArt";

/** A cell of the art, in CSS pixels: a monospaced glyph is about half as wide as it is tall. */
const CELL_WIDTH = 5;
const CELL_HEIGHT = 8;
/** Steps of brightness the glyphs are drawn in. */
const STEPS = 6;

/** The picture, decoded once for the window. */
let decoded: Promise<HTMLImageElement> | null = null;
const readPicture = () =>
  (decoded ??= new Promise((done, fail) => {
    const image = new Image();
    image.onload = () => done(image);
    image.onerror = () => {
      decoded = null;
      fail(new Error("The picture could not be read."));
    };
    image.src = picture;
  }));

/** Paints the picture as glyphs in the canvas's own color (the accent). Once per size. */
function paint(
  canvas: HTMLCanvasElement,
  image: HTMLImageElement,
  width: number,
  height: number,
): boolean {
  const cols = Math.ceil(width / CELL_WIDTH);
  const rows = Math.ceil(height / CELL_HEIGHT);
  const small = document.createElement("canvas");
  small.width = cols;
  small.height = rows;
  const reader = small.getContext("2d", { willReadFrequently: true });
  const ink = canvas.getContext("2d");
  if (!reader || !ink) return false;
  const crop = coverCrop(image.naturalWidth, image.naturalHeight, width, height);
  reader.imageSmoothingQuality = "high";
  reader.drawImage(image, crop.x, crop.y, crop.width, crop.height, 0, 0, cols, rows);
  const cells = asciiCells(reader.getImageData(0, 0, cols, rows).data, cols, rows);
  const scale = window.devicePixelRatio || 1;
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  ink.scale(scale, scale);
  ink.font = `${CELL_HEIGHT}px ui-monospace, Menlo, Consolas, monospace`;
  ink.textBaseline = "top";
  ink.fillStyle = getComputedStyle(canvas).color;
  for (let step = 1; step <= STEPS; step++) {
    ink.globalAlpha = 0.2 + (0.8 * step) / STEPS;
    for (let at = 0; at < cells.length; at++) {
      const cell = cells[at];
      if (cell.glyph === " " || Math.ceil(cell.light * STEPS) !== step) continue;
      ink.fillText(cell.glyph, (at % cols) * CELL_WIDTH, Math.floor(at / cols) * CELL_HEIGHT);
    }
  }
  return true;
}

/**
 * A picture drawn in glyphs, in the user's accent color, as in Echo: it fades
 * in once, then nothing moves. Decoration only.
 */
export function MailArt({ children }: { children?: React.ReactNode }) {
  const canvas = useRef<HTMLCanvasElement>(null);
  const [drawn, setDrawn] = useState(false);

  useEffect(() => {
    const element = canvas.current;
    if (!element) return;
    let gone = false;
    let size = "";
    let observer: ResizeObserver | undefined;
    void readPicture().then(
      (image) => {
        if (gone) return;
        const draw = () => {
          const width = element.clientWidth;
          const height = element.clientHeight;
          if (!width || !height || `${width}x${height}` === size) return;
          size = `${width}x${height}`;
          if (paint(element, image, width, height)) setDrawn(true);
        };
        draw();
        observer = new ResizeObserver(draw);
        observer.observe(element);
      },
      () => undefined,
    );
    return () => {
      gone = true;
      observer?.disconnect();
    };
  }, []);

  return (
    <div aria-hidden={children ? undefined : true} className="relative h-full min-h-0 w-full overflow-hidden bg-accent/5">
      <canvas
        ref={canvas}
        data-mail-art
        className={`absolute inset-0 size-full text-accent transition-opacity duration-500 motion-reduce:transition-none ${
          drawn ? "opacity-100" : "opacity-0"
        }`}
      />
      <div className="absolute inset-x-0 bottom-0 h-1/3 bg-gradient-to-t from-background-base to-transparent" />
      {children ? (
        <div className="absolute inset-x-0 bottom-0 p-6">{children}</div>
      ) : null}
    </div>
  );
}
