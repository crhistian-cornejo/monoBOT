import type {
  Attachment,
  GeneratedImageMeta,
} from "../../sessions/model/session";
import type {
  HarnessEvent,
  SendTurnInput,
} from "../../../integrations/harness/core/types";

/** What a Mono asks for with `app images.generate`. */
export type ImageRequest = {
  prompt: string;
  /** Takes to make, 1 to 4. */
  count: number;
  aspect?: string;
  /** An image to edit, by absolute path. */
  source?: string;
};

export const MAX_TAKES = 4;
const ASPECTS = new Set([
  "1:1",
  "3:2",
  "2:3",
  "4:3",
  "3:4",
  "16:9",
  "9:16",
  "21:9",
]);

/** Reads `images.generate` input, or says what is wrong with it. */
export function parseImageRequest(
  input: Record<string, unknown>,
): ImageRequest {
  const prompt = typeof input.prompt === "string" ? input.prompt.trim() : "";
  if (!prompt || prompt.length > 8_000)
    throw new Error("prompt must be a non-empty string under 8000 characters");
  const count = input.count === undefined ? 1 : Number(input.count);
  if (!Number.isInteger(count) || count < 1 || count > MAX_TAKES)
    throw new Error(`count must be a whole number from 1 to ${MAX_TAKES}`);
  const aspect = input.aspect === undefined ? undefined : String(input.aspect);
  if (aspect !== undefined && !ASPECTS.has(aspect))
    throw new Error(`aspect must be one of ${[...ASPECTS].join(", ")}`);
  const source = input.source === undefined ? undefined : String(input.source);
  if (source !== undefined && !/^(\/|[A-Za-z]:[\\/])/.test(source))
    throw new Error("source must be the absolute path of an image");
  return {
    prompt,
    count,
    ...(aspect ? { aspect } : {}),
    ...(source ? { source } : {}),
  };
}

/** The turn Gemini gets: make exactly the pictures asked for, nothing else. */
export function imageTurnText(request: ImageRequest): string {
  const takes =
    request.count === 1
      ? "one picture"
      : `${request.count} pictures, each a different take on the brief`;
  return [
    `Use your generate_image tool to make ${takes}. Do nothing else: no files, no shell, no questions; when the pictures are made, reply with one short line.`,
    request.aspect ? `Aspect ratio: ${request.aspect}.` : "",
    request.source
      ? `Edit the attached image (${request.source}); keep what the brief does not change.`
      : "",
    `Brief:\n${request.prompt}`,
  ]
    .filter(Boolean)
    .join("\n\n");
}

/** Only the picture tool runs unattended; anything else Gemini tries is refused. */
export function allowsImageTool(event: {
  title?: string;
  kind?: string;
}): boolean {
  return /generate_image|image[-_ ]?generat|nano.?banana/i.test(
    `${event.title ?? ""} ${event.kind ?? ""}`,
  );
}

export type ImageTurnRunner = {
  send(input: SendTurnInput): Promise<void>;
  approve(sessionId: string, requestId: number, allow: boolean): void;
  forget(sessionId: string): Promise<void>;
};

/**
 * Runs one Antigravity turn on its own throwaway session and returns the
 * pictures it made. The provider collects them from Gemini's folder.
 */
export async function runImageTurn(
  request: ImageRequest,
  options: {
    cwd: string;
    model: string;
    runner: ImageTurnRunner;
    source?: Attachment;
    sessionId?: string;
  },
): Promise<GeneratedImageMeta[]> {
  const sessionId = options.sessionId ?? `image-job-${crypto.randomUUID()}`;
  const images: GeneratedImageMeta[] = [];
  let failure: string | undefined;
  const onEvent = (event: HarnessEvent) => {
    if (event.type === "image.generated" && "path" in event) {
      images.push({
        path: event.path,
        name: event.name,
        mimeType: event.mimeType,
        size: event.size,
      });
    } else if (event.type === "approval.requested") {
      options.runner.approve(
        sessionId,
        event.requestId,
        allowsImageTool(event),
      );
    } else if (event.type === "session.error") {
      failure = event.message;
    }
  };
  try {
    await options.runner.send({
      sessionId,
      cwd: options.cwd,
      model: options.model,
      runtimeMode: "supervised",
      ephemeral: true,
      text: imageTurnText(request),
      ...(options.source ? { attachments: [options.source] } : {}),
      onEvent,
    });
  } finally {
    void options.runner.forget(sessionId).catch(() => undefined);
  }
  if (!images.length)
    throw new Error(
      failure ??
        "Gemini made no pictures. Check that Antigravity (agy) is installed and signed in.",
    );
  return images.slice(0, Math.max(request.count, 1));
}
