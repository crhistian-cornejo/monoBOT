import { useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { File, Shield } from "../../../shared/ui/icons";
import type { MailAttachment } from "../model/mail";

function bytes(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`;
  return `${(size / 1024 / 1024).toFixed(1)} MB`;
}

const VERDICT = {
  safe: { label: "Checked", className: "text-content/45" },
  caution: { label: "Careful", className: "text-accent" },
  danger: { label: "Blocked", className: "text-red-400" },
} as const;

/**
 * A message's files, in place. Rust looked at each one's bytes as the message
 * opened: pictures show from memory, the rest say what was found. Nothing is
 * saved to disk until the user presses Open, and a blocked file can't be.
 */
export function MailAttachments({
  attachments,
  onOpen,
}: {
  attachments: MailAttachment[];
  /** Saves the file to the app's cache and opens it with the system's app. */
  onOpen: (attachment: MailAttachment) => void;
}) {
  const [zoomed, setZoomed] = useState<MailAttachment>();
  const pictures = attachments.filter((file) => file.preview);
  const files = attachments.filter((file) => !file.preview);
  if (!attachments.length) return null;

  return (
    <section aria-label="Attachments" className="mt-3 flex flex-col gap-2">
      {pictures.length ? (
        <ul className="flex flex-wrap gap-2">
          {pictures.map((file) => (
            <li key={file.index}>
              <button
                type="button"
                title={`${file.name} · ${bytes(file.size)}`}
                onClick={() => setZoomed(file)}
                className="block overflow-hidden rounded-md border border-content/10 hover:border-content/25"
              >
                <img src={file.preview} alt={file.name} className="h-24 max-w-48 object-cover" />
              </button>
            </li>
          ))}
        </ul>
      ) : null}

      {files.length ? (
        <ul className="flex flex-wrap gap-2">
          {files.map((file) => {
            const verdict = VERDICT[file.risk.level];
            const blocked = file.risk.level === "danger";
            return (
              <li
                key={file.index}
                title={file.risk.reasons.join("\n") || "Nothing harmful found in its contents."}
                className={`flex max-w-80 items-center gap-2 rounded-md border px-2.5 py-1.5 text-[12px] ${
                  blocked ? "border-red-400/30 bg-red-400/5" : "border-content/10"
                }`}
              >
                <File className="size-4 shrink-0 text-content/45" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate">{file.name}</span>
                  <span className="flex items-center gap-1 text-[11px]">
                    <span className="text-content/40">{bytes(file.size)}</span>
                    <span className="text-content/25">·</span>
                    <span className={`flex items-center gap-0.5 ${verdict.className}`}>
                      {file.risk.level !== "safe" ? <Shield className="size-3" /> : null}
                      {verdict.label}
                    </span>
                  </span>
                  {file.risk.reasons.length ? (
                    <span className="block truncate text-[11px] text-content/50">
                      {file.risk.reasons[0]}
                    </span>
                  ) : null}
                </span>
                {blocked ? null : (
                  <button
                    type="button"
                    title="Save it to MonoBOT's cache and open it with your system's app"
                    onClick={() => onOpen(file)}
                    className="shrink-0 rounded-md px-2 py-1 text-content/70 hover:bg-content/8 hover:text-content"
                  >
                    Open
                  </button>
                )}
              </li>
            );
          })}
        </ul>
      ) : null}

      {zoomed?.preview ? (
        <Modal title={zoomed.name} size="lg" fitViewport onClose={() => setZoomed(undefined)}>
          <div className="grid place-items-center p-4">
            <img src={zoomed.preview} alt={zoomed.name} className="max-h-[70vh] max-w-full rounded-md object-contain" />
          </div>
        </Modal>
      ) : null}
    </section>
  );
}
