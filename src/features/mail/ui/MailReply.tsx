import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Send, Sparkles, Square, Undo2, X } from "../../../shared/ui/icons";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { homeDir } from "../../../platform/tauri/fs";
import {
  canRunHarnessTextPrompt,
  runHarnessTextPrompt,
} from "../../../integrations/harness/core/registry";
import { pickTextHarness } from "../../../integrations/harness/core/textHarness";
import { joinStreamText } from "../../../integrations/harness/core/streamText";
import { StreamedPlainText } from "../../sessions/ui/StreamedPlainText";
import {
  QUICK_CHANGES,
  cleanReply,
  mailText,
  nextGap,
  personLabel,
  replyGaps,
  replyPrompt,
  type MailMessage,
  type MailPerson,
  type MailRecipients,
  type OpenedMail,
} from "../model/mail";
import { mailApi } from "../model/mailApi";

/** Seconds to change one's mind after pressing Send. */
const UNDO_SECONDS = 5;

/** The box's text metrics, shared by the field and the streamed text so swapping them doesn't move a letter. */
const FIELD_TEXT =
  "min-h-20 max-h-[40vh] w-full rounded-md border px-2.5 py-2 text-[13px] leading-relaxed";

const TOOL_BUTTON =
  "flex h-7 shrink-0 items-center gap-1.5 rounded-md px-2 text-[12px] text-content/70 hover:bg-content/8 hover:text-content disabled:opacity-40";

/**
 * Asks a connected model for a reply; the user reviews it before anything is
 * sent. `onText` gets the reply so far while the model writes it.
 */
async function writeWithAI(input: {
  message: MailMessage;
  opened?: OpenedMail;
  ask?: string;
  draft?: string;
  signal: AbortSignal;
  onText: (text: string) => void;
}): Promise<string> {
  const harness = pickTextHarness();
  if (!canRunHarnessTextPrompt(harness))
    throw new Error("Connect an agent (Claude, Codex…) to write with AI.");
  // Only the last message counts: a model that says something before the
  // reply starts over with it.
  let streamed = "";
  let closed = false;
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
    onEvent: (event) => {
      if (input.signal.aborted) return;
      if (event.type === "message.completed") closed = true;
      if (event.type !== "message.delta") return;
      if (closed) {
        streamed = "";
        closed = false;
      }
      // Some agents resend the whole message so far: join, don't append.
      streamed = joinStreamText(streamed, event.text);
      const text = cleanReply(streamed);
      if (text) input.onText(text);
    },
  });
  const text = cleanReply(output);
  if (!text) throw new Error("The model wrote nothing. Try again.");
  return text;
}

function People({ label, people }: { label: string; people: MailPerson[] }) {
  return (
    <span className="flex min-w-0 items-center gap-1">
      <span className="shrink-0 text-content/45">{label}</span>
      {people.map((person) => (
        <span
          key={person.address}
          title={person.address}
          className="min-w-0 truncate rounded-md bg-content/6 px-1.5 py-0.5 text-content/80"
        >
          {personLabel(person)}
        </span>
      ))}
    </span>
  );
}

/**
 * The reply to a message, as a small card floating over it. AI writes into it
 * the way a chat reply streams, and the user edits it and sends it with their
 * own press, with a few seconds to undo. Who it goes to is Rust's to decide.
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
  const [ask, setAsk] = useState("");
  const [writing, setWriting] = useState(false);
  /** What AI has written so far this run, shown as it streams. */
  const [streamText, setStreamText] = useState<string>();
  /** The streamed text stays up until its last word has settled. */
  const [streamShown, setStreamShown] = useState(false);
  /** What the box held before AI last wrote in it, to undo to. */
  const [before, setBefore] = useState<string>();
  /** The request AI last wrote for, to try again with. */
  const [lastAsk, setLastAsk] = useState<string>();
  const [people, setPeople] = useState<MailRecipients>();
  const [all, setAll] = useState(false);
  const [status, setStatus] = useState<{ text: string; error?: boolean }>();
  const [countdown, setCountdown] = useState<number>();
  const abort = useRef<AbortController | undefined>(undefined);
  const sendTimer = useRef<number | undefined>(undefined);
  const frame = useRef<number | undefined>(undefined);
  const box = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    box.current?.focus();
    return () => {
      abort.current?.abort();
      window.clearInterval(sendTimer.current);
      if (frame.current != null) cancelAnimationFrame(frame.current);
    };
  }, []);

  useEffect(() => {
    let alive = true;
    setPeople(undefined);
    setAll(false);
    void mailApi
      .recipients(message.id, true)
      .then((found) => alive && setPeople(found))
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, [message.id]);

  // The box grows with its text up to its max height, then scrolls.
  useLayoutEffect(() => {
    const el = box.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight + 2}px`;
  }, [text, streamShown]);

  const gaps = useMemo(() => (streamShown ? [] : replyGaps(text)), [text, streamShown]);
  const to = people?.to.length ? people.to : [{ name: message.from, address: message.address }];
  const copies = people?.cc ?? [];

  const cancelPaint = () => {
    if (frame.current != null) cancelAnimationFrame(frame.current);
    frame.current = undefined;
  };

  /** `retry` writes again from what the box held before AI's last try. */
  const write = async (request?: string, retry = false) => {
    abort.current?.abort();
    const controller = new AbortController();
    abort.current = controller;
    const original = retry && before !== undefined ? before : text;
    setWriting(true);
    setStreamText("");
    setStreamShown(true);
    setStatus(undefined);
    setLastAsk(request);
    let latest = "";
    const paint = (next: string) => {
      latest = next;
      if (frame.current != null) return;
      frame.current = requestAnimationFrame(() => {
        frame.current = undefined;
        if (!controller.signal.aborted) setStreamText(latest);
      });
    };
    try {
      const written = await writeWithAI({
        message,
        opened,
        ask: request,
        draft: original,
        signal: controller.signal,
        onText: paint,
      });
      if (controller.signal.aborted) return;
      cancelPaint();
      setStreamText(written);
      setText(written);
      setBefore(original);
    } catch (failure) {
      if (controller.signal.aborted) return;
      cancelPaint();
      setStreamShown(false);
      setStatus({ text: failure instanceof Error ? failure.message : String(failure), error: true });
    } finally {
      if (abort.current === controller) setWriting(false);
    }
  };

  /** Stops the model and keeps what it wrote so far, still undoable. */
  const stop = () => {
    const controller = abort.current;
    if (!controller || !writing) return;
    controller.abort();
    cancelPaint();
    if (streamText) {
      setBefore(text);
      setText(streamText);
    }
    setWriting(false);
    setStreamShown(false);
    requestAnimationFrame(() => box.current?.focus());
  };

  /** The last word has settled: the text becomes the editable box in its place. */
  const settle = () => {
    setStreamShown(false);
    requestAnimationFrame(() => {
      const el = box.current;
      if (!el) return;
      el.focus();
      el.setSelectionRange(el.value.length, el.value.length);
    });
  };

  const undoAI = () => {
    if (before === undefined) return;
    setText(before);
    setBefore(undefined);
    box.current?.focus();
  };

  const selectNextGap = () => {
    const el = box.current;
    if (!el) return;
    const gap = nextGap(gaps, el.selectionEnd);
    if (!gap) return;
    el.focus();
    el.setSelectionRange(gap.start, gap.end);
  };

  const busy = writing || streamShown;

  const send = () => {
    if (!text.trim() || countdown != null || busy) return;
    let left = UNDO_SECONDS;
    setCountdown(left);
    sendTimer.current = window.setInterval(() => {
      left -= 1;
      if (left > 0) return setCountdown(left);
      window.clearInterval(sendTimer.current);
      setCountdown(undefined);
      setStatus({ text: "Sending…" });
      void mailApi
        .send(message.id, text, all)
        .then(() => onSent())
        .catch((failure) => setStatus({ text: String(failure), error: true }));
    }, 1000);
  };

  const undoSend = () => {
    window.clearInterval(sendTimer.current);
    setCountdown(undefined);
    setStatus({ text: "Not sent." });
  };

  const saveDraft = async () => {
    try {
      await mailApi.draft(message.id, text, all);
      setStatus({ text: "Saved as a draft on your mail server." });
    } catch (failure) {
      setStatus({ text: String(failure), error: true });
    }
  };

  const line = () => {
    if (countdown != null)
      return <span className="text-content/55">Sending in {countdown}s…</span>;
    if (status)
      return (
        <span className={status.error ? "text-red-400" : "text-content/50"}>{status.text}</span>
      );
    if (before !== undefined && !busy)
      return (
        <span className="flex items-center gap-1 text-content/50">
          <Sparkles className="size-3 text-accent" />
          Written with AI
          <button
            type="button"
            onClick={undoAI}
            className="ml-1 flex items-center gap-1 rounded-md px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
          >
            <Undo2 className="size-3" /> Undo
          </button>
          <button
            type="button"
            onClick={() => void write(lastAsk, true)}
            className="rounded-md px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
          >
            Try again
          </button>
        </span>
      );
    return null;
  };
  const statusLine = line();

  return (
    <div
      role="dialog"
      aria-label={`Reply to ${message.from}`}
      className="flex flex-col gap-1.5 rounded-xl border border-content/10 bg-background-base p-2 shadow-xl"
    >
      <div className="flex min-w-0 items-center gap-2 px-1 text-[12px]">
        <div className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1">
          <People label="To" people={to} />
          {all && copies.length ? <People label="Cc" people={copies} /> : null}
        </div>
        {copies.length ? (
          <button
            type="button"
            aria-pressed={all}
            onClick={() => setAll((on) => !on)}
            title={
              all
                ? "Reply only to the sender"
                : `Also send a copy to ${copies.map(personLabel).join(", ")}`
            }
            className={`shrink-0 rounded-md px-1.5 py-0.5 ${
              all ? "bg-accent/12 text-accent" : "text-content/55 hover:bg-content/8 hover:text-content"
            }`}
          >
            Reply all
          </button>
        ) : null}
        <button
          type="button"
          title="Close (Esc)"
          aria-label="Close"
          onClick={onClose}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content"
        >
          <X className="size-3.5" />
        </button>
      </div>

      <div className={`relative rounded-md ${writing ? "ai-glow" : ""}`}>
        {streamShown ? (
          <>
            <StreamedPlainText
              text={streamText ?? ""}
              streaming={writing}
              onSettled={settle}
              className={`${FIELD_TEXT} overflow-y-auto border-transparent bg-background-base text-content/90`}
            />
            {!streamText ? (
              <span className="pointer-events-none absolute left-2.5 top-2 flex items-center gap-1.5 text-[13px]">
                <Sparkles className="size-3.5 text-accent" />
                <Shimmer>Reading the email…</Shimmer>
              </span>
            ) : null}
          </>
        ) : (
          <textarea
            ref={box}
            value={text}
            onChange={(event) => {
              setText(event.target.value);
              setStatus(undefined);
              // Once the user edits AI's text it is theirs: no undo over it.
              setBefore(undefined);
            }}
            onKeyDown={(event) => {
              if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                event.preventDefault();
                send();
              } else if (event.key === "Escape") {
                event.preventDefault();
                onClose();
              }
            }}
            rows={3}
            placeholder="Write your reply, or let AI draft it"
            className={`${FIELD_TEXT} block resize-none border-content/10 bg-background-base outline-none focus:border-content/25`}
          />
        )}
        {gaps.length ? (
          <button
            type="button"
            onClick={selectNextGap}
            title="Select the next blank to fill in"
            className="absolute bottom-1.5 right-1.5 rounded-md border border-accent/30 bg-background-base px-1.5 py-0.5 text-[11px] text-accent hover:bg-accent/10"
          >
            {gaps.length} to fill
          </button>
        ) : null}
      </div>

      <div className="flex flex-wrap items-center gap-1">
        <div className="relative flex h-7 min-w-36 flex-1 items-center">
          <Sparkles className="pointer-events-none absolute left-2 size-3.5 text-accent" />
          <input
            value={ask}
            onChange={(event) => setAsk(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !busy) {
                event.preventDefault();
                void write(ask || undefined);
              } else if (event.key === "Escape" && writing) {
                event.preventDefault();
                stop();
              }
            }}
            placeholder={text.trim() ? "Ask AI to change it" : "Ask AI to write it"}
            aria-label="What AI should write"
            className="h-7 w-full rounded-md bg-content/5 pl-7 pr-2 text-[12px] outline-none placeholder:text-content/40 focus:bg-content/8"
          />
        </div>
        {writing ? (
          <button type="button" onClick={stop} title="Stop (Esc)" className={TOOL_BUTTON}>
            <Square className="size-3" /> Stop
          </button>
        ) : (
          <button
            type="button"
            disabled={busy}
            onClick={() => void write(ask || undefined)}
            className={TOOL_BUTTON}
          >
            {text.trim() ? "Rewrite" : "Write"}
          </button>
        )}
        {text.trim() && !busy
          ? QUICK_CHANGES.map((change) => (
              <button
                key={change.label}
                type="button"
                onClick={() => void write(change.ask)}
                className={TOOL_BUTTON}
              >
                {change.label}
              </button>
            ))
          : null}
        <div className="ml-auto flex shrink-0 items-center gap-1">
          {countdown != null ? (
            <button type="button" onClick={undoSend} className={TOOL_BUTTON}>
              Undo
            </button>
          ) : (
            <>
              <button
                type="button"
                disabled={!text.trim() || busy}
                onClick={() => void saveDraft()}
                className={TOOL_BUTTON}
              >
                Draft
              </button>
              <button
                type="button"
                disabled={!text.trim() || busy}
                onClick={send}
                title="Send (Ctrl/⌘ + Enter)"
                className="primary-action flex h-7 items-center gap-1.5 rounded-md px-2.5 text-[12px] disabled:cursor-default"
              >
                <Send className="size-3.5" /> Send
              </button>
            </>
          )}
        </div>
      </div>

      {statusLine ? (
        <div
          role={status?.error ? "alert" : "status"}
          className="flex min-w-0 items-center truncate px-1 text-[11px]"
        >
          {statusLine}
        </div>
      ) : null}
    </div>
  );
}
