import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Send, Sparkles, Square, Undo2, X } from "../../../shared/ui/icons";
import { Shimmer } from "../../../shared/ui/Shimmer";
import { homeDir } from "../../../platform/tauri/fs";
import {
  canRunHarnessTextPrompt,
  runHarnessTextPrompt,
} from "../../../integrations/harness/core/registry";
import * as textHarness from "../../../integrations/harness/core/textHarness";
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
  replySubject,
  splitSubject,
  type MailAccount,
  type MailMessage,
  type MailRecipients,
  type OpenedMail,
} from "../model/mail";
import { mailApi } from "../model/mailApi";
import { MailPeopleRow, MailPersonChip } from "./MailPeople";

/** Seconds to change one's mind after pressing Send. */
const UNDO_SECONDS = 5;

/** The box's text metrics, shared by the field and the streamed text so swapping them doesn't move a letter. */
const FIELD_TEXT = "min-h-20 max-h-[40vh] w-full px-2.5 py-2 text-[13px] leading-relaxed";

/** The subject's, large over the text, shared the same way. */
const SUBJECT_TEXT = "block w-full px-2.5 pt-2.5 text-[17px] font-medium leading-snug text-content";

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
  draftSubject?: string;
  signal: AbortSignal;
  onText: (text: string) => void;
  /** The model finished its message (the agent may take a moment more to exit), or started another. */
  onArrived?: (arrived: boolean) => void;
}): Promise<string> {
  const routed =
    typeof textHarness.routeTextHarnesses === "function"
      ? textHarness.routeTextHarnesses()
      : [textHarness.pickTextHarness()];
  const candidates = routed.filter((id) => canRunHarnessTextPrompt(id));
  if (candidates.length === 0)
    throw new Error("Connect an agent (Claude, Codex…) to write with AI.");
  const cwd = await homeDir();
  const prompt = replyPrompt({
    from: `${input.message.from} <${input.message.address}>`,
    subject: input.message.subject,
    body: mailText(input.message, input.opened),
    ask: input.ask,
    draft: input.draft,
    draftSubject: input.draftSubject,
  });
  let lastError: unknown;
  for (let index = 0; index < candidates.length; index += 1) {
    const harness = candidates[index];
    if (input.signal.aborted) break;
    if (index > 0) {
      input.onArrived?.(false);
      input.onText("");
    }
    // Only the last message counts: a model that says something before the
    // reply starts over with it.
    let streamed = "";
    let closed = false;
    try {
      const output = await runHarnessTextPrompt({
        harness,
        cwd,
        prompt,
        timeoutMs: 90_000,
        signal: input.signal,
        onEvent: (event) => {
          if (input.signal.aborted) return;
          if (event.type === "message.completed") {
            closed = true;
            input.onArrived?.(true);
          }
          if (event.type !== "message.delta") return;
          if (closed) {
            streamed = "";
            closed = false;
            input.onArrived?.(false);
          }
          // Some agents resend the whole message so far: join, don't append.
          streamed = joinStreamText(streamed, event.text);
          const text = cleanReply(streamed);
          if (text) input.onText(text);
        },
      });
      const text = cleanReply(output);
      if (!text || !splitSubject(text).body.trim())
        throw new Error("The model wrote nothing. Try again.");
      return text;
    } catch (failure) {
      lastError = failure;
      if (input.signal.aborted) throw failure;
    }
  }
  throw lastError instanceof Error
    ? lastError
    : new Error(String(lastError ?? "Connect an agent (Claude, Codex…) to write with AI."));
}

/**
 * The reply to a message, as a small card floating over it. AI writes into it
 * the way a chat reply streams, and the user edits it and sends it with their
 * own press, with a few seconds to undo. Who it goes to is Rust's to decide.
 * AI also writes a subject, shown large over the text; it goes with the reply.
 */
export function MailReply({
  message,
  account,
  opened,
  onClose,
  onSent,
}: {
  message: MailMessage;
  /** The user's account the reply goes from. */
  account?: MailAccount;
  opened?: OpenedMail;
  onClose: () => void;
  onSent: () => void;
}) {
  const [text, setText] = useState("");
  const [subject, setSubject] = useState(() => replySubject(message.subject));
  const [ask, setAsk] = useState("");
  const [writing, setWriting] = useState(false);
  /** What AI has written so far this run, shown as it streams. */
  const [streamText, setStreamText] = useState<string>();
  /** The streamed text stays up until its last word has settled. */
  const [streamShown, setStreamShown] = useState(false);
  /** The model finished its message, though its agent may not have exited yet. */
  const [arrived, setArrived] = useState(false);
  /** Words of the streamed text still coming onto the screen. */
  const [revealing, setRevealing] = useState(false);
  /** What the box and the subject held before AI last wrote in them, to undo to. */
  const [before, setBefore] = useState<{ text: string; subject: string }>();
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
  /** What AI has written so far, its subject apart from its text. */
  const streamed = useMemo(() => splitSubject(streamText ?? ""), [streamText]);
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
    const original = retry && before !== undefined ? before : { text, subject };
    setWriting(true);
    setArrived(false);
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
        draft: original.text,
        draftSubject: original.subject,
        signal: controller.signal,
        onText: paint,
        onArrived: (on) => {
          if (!controller.signal.aborted) setArrived(on);
        },
      });
      if (controller.signal.aborted) return;
      cancelPaint();
      const reply = splitSubject(written);
      setStreamText(written);
      setText(reply.body);
      if (reply.subject) setSubject(reply.subject);
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
    if (streamed.body) {
      setBefore({ text, subject });
      setText(streamed.body);
      if (streamed.subject) setSubject(streamed.subject);
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
    setText(before.text);
    setSubject(before.subject);
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
  /** The border lights while words are still coming, and goes out with the last of them. */
  const glowing = writing && !(arrived && !revealing);
  const aiWrote = before !== undefined && !busy && countdown == null && !status;

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
        .send(message.id, text, all, subject)
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
      await mailApi.draft(message.id, text, all, subject);
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
    return null;
  };
  const statusLine = line();

  return (
    <div
      role="dialog"
      aria-label={`Reply to ${message.from}`}
      className="flex flex-col gap-1.5 rounded-xl border border-content/10 bg-background-base p-2 shadow-xl"
    >
      <div className="flex flex-col divide-y divide-content/8 rounded-md border border-content/10">
        <MailPeopleRow
          label="From"
          end={
            <button
              type="button"
              title="Close (Esc)"
              aria-label="Close"
              onClick={onClose}
              className="grid size-6 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content"
            >
              <X className="size-3.5" />
            </button>
          }
        >
          {account ? (
            <MailPersonChip own person={{ name: account.name, address: account.address }} />
          ) : (
            <span className="text-content/45">Your account</span>
          )}
        </MailPeopleRow>
        <MailPeopleRow
          label="To"
          end={
            copies.length ? (
              <button
                type="button"
                aria-pressed={all}
                onClick={() => setAll((on) => !on)}
                title={
                  all
                    ? "Reply only to the sender"
                    : `Also send a copy to ${copies.map(personLabel).join(", ")}`
                }
                className={`rounded-md px-1.5 py-0.5 ${
                  all ? "bg-accent/12 text-accent" : "text-content/55 hover:bg-content/8 hover:text-content"
                }`}
              >
                Reply all
              </button>
            ) : null
          }
        >
          {to.map((person) => (
            <MailPersonChip key={person.address} person={person} />
          ))}
        </MailPeopleRow>
        {all && copies.length ? (
          <MailPeopleRow label="Cc">
            {copies.map((person) => (
              <MailPersonChip key={person.address} person={person} />
            ))}
          </MailPeopleRow>
        ) : null}
      </div>

      <div
        className={`relative rounded-md border ${
          glowing ? "ai-glow border-transparent" : "border-content/10 focus-within:border-content/25"
        }`}
      >
        {streamShown ? (
          <>
            {streamed.subject ? (
              <StreamedPlainText
                text={streamed.subject}
                streaming={writing}
                className={SUBJECT_TEXT}
              />
            ) : !streamed.body ? (
              <span className={SUBJECT_TEXT}>
                <span className="flex items-center gap-1.5 text-[13px] font-normal">
                  <Sparkles className="size-3.5 text-accent" />
                  <Shimmer>Reading the email…</Shimmer>
                </span>
              </span>
            ) : (
              <span className={SUBJECT_TEXT}>{subject}</span>
            )}
            <StreamedPlainText
              text={streamed.body}
              streaming={writing}
              onSettled={settle}
              onRevealing={setRevealing}
              className={`${FIELD_TEXT} overflow-y-auto text-content/90`}
            />
          </>
        ) : (
          <>
            <input
              value={subject}
              onChange={(event) => {
                setSubject(event.target.value);
                setStatus(undefined);
                setBefore(undefined);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  box.current?.focus();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  onClose();
                }
              }}
              placeholder="Subject"
              aria-label="Subject"
              className={`${SUBJECT_TEXT} bg-transparent outline-none placeholder:text-content/35`}
            />
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
              className={`${FIELD_TEXT} block resize-none bg-transparent outline-none ${
                aiWrote || gaps.length ? "pb-10" : ""
              }`}
            />
          </>
        )}
        {aiWrote || gaps.length ? (
          <div className="pointer-events-none absolute inset-x-1.5 bottom-1.5 flex items-end gap-2">
            {aiWrote ? (
              <div className="pointer-events-auto flex min-w-0 items-center gap-0.5 rounded-md border border-content/10 bg-background-base py-0.5 pl-1.5 pr-0.5 text-[11px] text-content/55">
                <Sparkles className="size-3 shrink-0 text-accent" />
                <span className="mx-1 truncate">Written with AI</span>
                <button
                  type="button"
                  onClick={undoAI}
                  className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
                >
                  <Undo2 className="size-3" /> Undo
                </button>
                <button
                  type="button"
                  onClick={() => void write(lastAsk, true)}
                  className="shrink-0 rounded px-1.5 py-0.5 text-content/70 hover:bg-content/8 hover:text-content"
                >
                  Try again
                </button>
              </div>
            ) : null}
            {gaps.length ? (
              <button
                type="button"
                onClick={selectNextGap}
                title="Select the next blank to fill in"
                className="pointer-events-auto ml-auto shrink-0 rounded-md border border-accent/30 bg-background-base px-1.5 py-0.5 text-[11px] text-accent hover:bg-accent/10"
              >
                {gaps.length} to fill
              </button>
            ) : null}
          </div>
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
