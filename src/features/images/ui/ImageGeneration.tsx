import { useEffect, useState, type CSSProperties } from "react";
import { readBinaryFile } from "../../../platform/tauri/fs";
import { sniffImageMime } from "../../files/model/filePreview";
import { ImageLightbox } from "../../../shared/ui/ImageLightbox";
import { Shimmer } from "../../../shared/ui/Shimmer";
import type {
  GeneratedImageMeta,
  ImageJobMeta,
} from "../../sessions/model/session";

const CELLS = 6;
/** A job still "generating" this long after it started was cut off by a quit. */
const STALE_MS = 10 * 60_000;

/** The frame's CSS aspect ratio for a job's "16:9"-style aspect. */
export function frameRatio(aspect: string | undefined): string {
  const match = aspect?.match(/^(\d+):(\d+)$/);
  return match ? `${match[1]} / ${match[2]}` : "1 / 1";
}

/** How the job reads now: a run the app quit during can no longer finish. */
export function jobState(
  job: ImageJobMeta,
  now = Date.now(),
): ImageJobMeta["status"] | "interrupted" {
  if (job.status === "generating" && now - job.startedAt > STALE_MS)
    return "interrupted";
  return job.status;
}

/**
 * Pictures a Mono asked Gemini for. While they are made, one frame per take
 * whose cells breathe under a "Creating" pill; each picture then fades in
 * where its frame was.
 */
export function ImageGeneration({ job }: { job: ImageJobMeta }) {
  const state = jobState(job);
  const ratio = frameRatio(job.aspect);
  const frames =
    state === "done" ? job.images.length : Math.max(1, Math.min(job.count, 4));
  const columns = frames === 1 ? 1 : 2;
  const wide = ratio !== "1 / 1" && columns === 1;

  return (
    <figure className="min-w-0 px-4 pb-3 pt-2">
      <div
        className={`grid gap-2 ${columns === 1 ? "grid-cols-1" : "grid-cols-2"} ${
          wide
            ? "max-w-[520px]"
            : columns === 1
              ? "max-w-[320px]"
              : "max-w-[460px]"
        }`}
      >
        {Array.from({ length: frames }, (_, index) =>
          state === "done" && job.images[index] ? (
            <Picture
              key={job.images[index].path}
              image={job.images[index]}
              ratio={ratio}
              alt={job.prompt}
            />
          ) : (
            <Frame key={index} ratio={ratio} index={index} state={state} />
          ),
        )}
      </div>
      <figcaption
        title={job.prompt}
        className="mt-1.5 flex min-w-0 items-center gap-1.5 text-[11px] text-content/45"
      >
        <span className="shrink-0 text-content/60">Nano Banana</span>
        <span aria-hidden>·</span>
        {state === "failed" || state === "interrupted" ? (
          <span role="alert" className="min-w-0 truncate text-content/60">
            {state === "interrupted"
              ? "Stopped when MonoBOT closed. Ask again to retry."
              : (job.error ?? "Could not make the pictures.")}
          </span>
        ) : (
          <span className="min-w-0 truncate">{job.prompt}</span>
        )}
      </figcaption>
    </figure>
  );
}

function Frame({
  ratio,
  index,
  state,
}: {
  ratio: string;
  index: number;
  state: ReturnType<typeof jobState>;
}) {
  const making = state === "generating";
  return (
    <div
      role={making ? "status" : undefined}
      aria-label={making ? "Making a picture" : undefined}
      className="relative overflow-hidden rounded-xl border border-content/10 bg-content/[0.03]"
      style={{ aspectRatio: ratio }}
    >
      <div
        aria-hidden
        className="absolute inset-0 grid gap-px p-px"
        style={{
          gridTemplateColumns: `repeat(${CELLS}, 1fr)`,
          gridTemplateRows: `repeat(${CELLS}, 1fr)`,
        }}
      >
        {Array.from({ length: CELLS * CELLS }, (_, cell) => {
          const row = Math.floor(cell / CELLS);
          const column = cell % CELLS;
          return (
            <span
              key={cell}
              className={`rounded-[3px] bg-content/6 ${making ? "image-cell" : "opacity-40"}`}
              style={
                making
                  ? ({
                      animationDelay: `${(row + column) * 90 + index * 240}ms`,
                    } as CSSProperties)
                  : undefined
              }
            />
          );
        })}
      </div>
      {making ? (
        <span className="absolute bottom-2 left-1/2 -translate-x-1/2 rounded-full border border-content/10 bg-background-base/80 px-2.5 py-0.5 text-[11px] text-content/70 backdrop-blur-sm">
          <Shimmer>Creating</Shimmer>
        </span>
      ) : null}
    </div>
  );
}

function Picture({
  image,
  ratio,
  alt,
}: {
  image: GeneratedImageMeta;
  ratio: string;
  alt: string;
}) {
  const [url, setUrl] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [shown, setShown] = useState(false);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let created: string | null = null;
    void readBinaryFile(image.path).then(
      (bytes) => {
        if (cancelled) return;
        const mime = sniffImageMime(bytes) ?? image.mimeType;
        created = URL.createObjectURL(new Blob([bytes], { type: mime }));
        setUrl(created);
      },
      () => !cancelled && setFailed(true),
    );
    return () => {
      cancelled = true;
      if (created) URL.revokeObjectURL(created);
    };
  }, [image.path, image.mimeType]);

  if (failed)
    return (
      <div
        role="alert"
        className="flex items-center justify-center rounded-xl border border-content/10 p-3 text-center text-[11px] text-content/50"
        style={{ aspectRatio: ratio }}
      >
        Could not open {image.name}
      </div>
    );

  return (
    <>
      <button
        type="button"
        aria-label={`Open ${image.name} full screen`}
        title={image.name}
        onClick={() => url && setOpen(true)}
        className="relative block cursor-zoom-in overflow-hidden rounded-xl border border-content/10 bg-content/[0.03] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        style={{ aspectRatio: ratio }}
      >
        {url ? (
          <img
            src={url}
            alt={alt}
            draggable={false}
            onLoad={() => setShown(true)}
            className={`size-full object-cover transition-[opacity,filter] duration-500 ease-out motion-reduce:transition-none ${
              shown ? "opacity-100 blur-0" : "opacity-0 blur-sm"
            }`}
          />
        ) : null}
      </button>
      {open && url ? (
        <ImageLightbox src={url} alt={alt} onClose={() => setOpen(false)} />
      ) : null}
    </>
  );
}
