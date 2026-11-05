import { useEffect, useRef, useState } from "react";
import { Send, Sparkles } from "../../../shared/ui/icons";
import { homeDir } from "../../../platform/tauri/fs";
import {
  canRunHarnessTextPrompt,
  runHarnessTextPrompt,
} from "../../../integrations/harness/core/registry";
import { pickTextHarness } from "../../../integrations/harness/core/textHarness";
import {
  QUICK_CHANGES,
  cleanReply,
  mailText,
  replyPrompt,
  type MailMessage,
  type OpenedMail,
} from "../model/mail";
import { mailApi } from "../model/mailApi";

/** Seconds to change one's mind after pressing Send. */
const UNDO_SECONDS = 5;

/** Asks a connected model for a reply; the user reviews it before anything is sent. */
async function writeWithAI(input: {
  message: MailMessage;
  opened?: OpenedMail;
  ask?: string;
  draft?: string;
  signal: AbortSignal;
}): Promise<string> {
  const harness = pickTextHarness();
  if (!canRunHarnessTextPrompt(harness))
    throw new Error("Connect an agent (Claude, Codex…) to write with AI.");
  const output = await runHarnessTextPrompt({
    harness,
    cwd: await homeDir(),
    prompt: replyPrompt({
      from: `${input.message.from} <${input.message.address}>`,
      subject: input.message.subject,
      body: mailText(input.message, input.opened),
      ask: input.ask,
      draft: input.draft,
    }),
    timeoutMs: 90_000,
    signal: input.signal,
  });
  const text = cleanReply(output);
  if (!text) throw new Error("The model wrote nothing. Try again.");
  return text;
}

/**
 * The reply to the sender. AI only fills the box: the user edits it and sends
 * it with their own press, with a few seconds to undo.
 */
export function MailReply({
  message,
  opened,
  onClose,
  onSent,
}: {
  message: MailMessage;
  opened?: OpenedMail;
  onClose: () => void;
  onSent: () => void;
}) {
  const [text, setText] = useState("");
  /** The AI's proposal, shown over the text until accepted. */
  const [proposal, setProposal] = useState<{ text: string; before: string }>();
  const [ask, setAsk] = useState("");
  const [writing, setWriting] = useState(false);
  const [status, setStatus] = useState<{ text: string; error?: boolean }>();
  const [countdown, setCountdown] = useState<number>();
  const abort = useRef<AbortController | undefined>(undefined);
  const sendTimer = useRef<number | undefined>(undefined);
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    box.current?.focus();
    return () => {
      abort.current?.abort();
      window.clearInterval(sendTimer.current);
    };
  }, []);

  const write = async (request?: string) => {
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;
    setWriting(true);
    setStatus(undefined);
    const before = text;
    try {
      const written = await writeWithAI({
        message,
        opened,
        ask: request,
        draft: before,
        signal: controller.signal,
      });
      if (!controller.signal.aborted) setProposal({ text: written, before });
    } catch (failure) {
      if (!controller.signal.aborted)
        setStatus({ text: failure instanceof Error ? failure.message : String(failure), error: true });
    } finally {
      if (abort.current === controller) setWriting(false);
    }
  };

  const accept = () => {
    if (!proposal) return;
    setText(proposal.text);
    setProposal(undefined);
    box.current?.focus();
  };

  const send = () => {
    if (!text.trim() || countdown != null) return;
    let left = UNDO_SECONDS;
    setCountdown(left);
    sendTimer.current = window.setInterval(() => {
      left -= 1;
      if (left > 0) return setCountdown(left);
      window.clearInterval(sendTimer.current);
      setCountdown(undefined);
      setStatus({ text: "Sending…" });
      void mailApi
        .send(message.id, text)
        .then(() => onSent())
        .catch((failure) => setStatus({ text: String(failure), error: true }));
    }, 1000);
  };

  const undo = () => {
    window.clearInterval(sendTimer.current);
    setCountdown(undefined);
    setStatus({ text: "Not sent." });
  };

  const saveDraft = async () => {
    try {
      await mailApi.draft(message.id, text);
      setStatus({ text: "Saved as a draft on your mail server." });
    } catch (failure) {
      setStatus({ text: String(failure), error: true });
    }
  };

  return (
    <div className="flex flex-col gap-2 rounded-lg border border-content/10 bg-background-base p-3">
      <div className="flex items-center justify-between text-[12px] text-content/60">
        <span className="min-w-0 truncate">
          To {message.from} &lt;{message.address}&gt;
        </span>
        <button
          type="button"
          onClick={onClose}
          className="rounded-md px-2 py-1 hover:bg-content/8"
        >
          Close
        </button>
      </div>

      {proposal ? (
        <div className="rounded-md border border-accent/30 bg-accent/6 p-2.5">
          <p className="mb-1 flex items-center gap-1.5 text-[11px] text-accent">
            <Sparkles className="size-3" /> Proposed reply
          </p>
          <p className="whitespace-pre-wrap text-[13px]">{proposal.text}</p>
          <div className="mt-2 flex gap-1">
            <button type="button" onClick={accept} className="primary-action rounded-md px-3 py-1 text-[12px]">
              Use it
            </button>
            <button
              type="button"
              onClick={() => setProposal(undefined)}
              className="rounded-md px-3 py-1 text-[12px] hover:bg-content/8"
            >
              Discard
            </button>
            <button
              type="button"
              disabled={writing}
              onClick={() => void write(ask || undefined)}
              className="rounded-md px-3 py-1 text-[12px] hover:bg-content/8"
            >
              Another
            </button>
          </div>
        </div>
      ) : null}

      <textarea
        ref={box}
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            send();
          } else if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
        rows={7}
        placeholder="Write your reply, or let AI draft it"
        className="w-full resize-y rounded-md border border-content/10 bg-background-base px-2.5 py-2 text-[13px] outline-none focus:border-content/25"
      />

      <div className="flex flex-wrap items-center gap-1.5">
        <input
          value={ask}
          onChange={(event) => setAsk(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") {
              event.preventDefault();
              void write(ask || undefined);
            }
          }}
          placeholder="What should it say? (optional)"
          className="h-8 min-w-40 flex-1 rounded-md border border-content/10 bg-background-base px-2.5 text-[12px] outline-none focus:border-content/25"
        />
        <button
          type="button"
          disabled={writing}
          onClick={() => void write(ask || undefined)}
          className="flex h-8 items-center gap-1.5 rounded-md px-3 text-[12px] hover:bg-content/8 disabled:opacity-50"
        >
          <Sparkles className="size-3.5 text-accent" />
          {writing ? "Writing…" : text.trim() ? "Rewrite with AI" : "Write with AI"}
        </button>
        {text.trim()
          ? QUICK_CHANGES.map((change) => (
              <button
                key={change.label}
                type="button"
                disabled={writing}
                onClick={() => void write(change.ask)}
                className="h-8 rounded-md px-2.5 text-[12px] text-content/70 hover:bg-content/8 disabled:opacity-50"
              >
                {change.label}
              </button>
            ))
          : null}
      </div>

      <div className="flex items-center justify-between gap-2">
        <p
          role={status?.error ? "alert" : "status"}
          className={`min-w-0 truncate text-[12px] ${status?.error ? "text-red-400" : "text-content/50"}`}
        >
          {countdown != null ? `Sending in ${countdown}s…` : status?.text}
        </p>
        <div className="flex shrink-0 gap-1.5">
          {countdown != null ? (
            <button type="button" onClick={undo} className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8">
              Undo
            </button>
          ) : (
            <>
              <button
                type="button"
                disabled={!text.trim()}
                onClick={() => void saveDraft()}
                className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 disabled:opacity-40"
              >
                Save draft
              </button>
              <button
                type="button"
                disabled={!text.trim()}
                onClick={send}
                title="Send (Ctrl/⌘ + Enter)"
                className="primary-action flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[12px] disabled:cursor-default"
              >
                <Send className="size-3.5" /> Send
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
