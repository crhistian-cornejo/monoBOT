import { useEffect, useRef, useState } from "react";
import { copyMessage } from "../../../platform/tauri/clipboard";
import { Check, Copy, FilePlusCorner } from "../../../shared/ui/icons";
import { playCue } from "../../settings/model/sounds";
import type { Attachment } from "../model/session";

/** Copies a reply or message, confirming with a check for two seconds. */
export function CopyTurnButton({
  text,
  attachments,
  label = "Copy response",
}: {
  text: string;
  attachments?: Attachment[];
  label?: string;
}) {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    setError(null);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [text, attachments]);

  return (
    <>
      <button
        type="button"
        disabled={pending}
        title={copied ? "Copied" : label}
        aria-label={copied ? "Copied" : label}
        className="-ml-1 rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70"
        onClick={(event) => {
          event.stopPropagation();
          setError(null);
          setCopied(false);
          setPending(true);
          playCue("copy");
          void copyMessage(text, attachments).then(
            () => {
              setPending(false);
              setCopied(true);
              if (timer.current != null) window.clearTimeout(timer.current);
              timer.current = window.setTimeout(() => setCopied(false), 2000);
            },
            (error: unknown) => {
              setPending(false);
              setError(error instanceof Error ? error.message : String(error));
            },
          );
        }}
      >
        {copied ? (
          <Check className="size-3.5" strokeWidth={1.75} />
        ) : (
          <Copy className="size-3.5" strokeWidth={1.75} />
        )}
      </button>
      {error && (
        <span role="alert" className="max-w-xs text-xs text-content/70">
          Copy failed. {error}
        </span>
      )}
    </>
  );
}

/** Saves a reply or message to Notes, confirming with a check. */
export function SaveNoteButton({
  text,
  onSave,
}: {
  text: string;
  onSave: (text: string) => void | Promise<void>;
}) {
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(false);
  const [saved, setSaved] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setSaved(false);
    setError(null);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [text]);

  return (
    <>
      <button
        type="button"
        disabled={pending}
        title={saved ? "Saved to Notes" : "Save as note"}
        aria-label={saved ? "Saved to Notes" : "Save as note"}
        className="rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70"
        onClick={async (event) => {
          event.stopPropagation();
          setError(null);
          setSaved(false);
          setPending(true);
          try {
            await onSave(text);
            playCue("copy");
            setSaved(true);
            if (timer.current != null) window.clearTimeout(timer.current);
            timer.current = window.setTimeout(() => setSaved(false), 2000);
          } catch (error) {
            setError(error instanceof Error ? error.message : String(error));
          } finally {
            setPending(false);
          }
        }}
      >
        {saved ? (
          <Check className="size-3.5" strokeWidth={1.75} />
        ) : (
          <FilePlusCorner className="size-3.5" strokeWidth={1.75} />
        )}
      </button>
      {error && (
        <span role="alert" className="max-w-xs text-xs text-content/70">
          Could not save note. {error}
        </span>
      )}
    </>
  );
}
