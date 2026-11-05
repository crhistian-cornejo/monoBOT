import {
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { ArrowUp, Plus, SmilePlus } from "../../../shared/ui/icons";
import { useDictation } from "../../dictation/ui/useDictation";
import { DictationBar, DictationButton } from "../../dictation/ui/Dictation";
import { insertDictated } from "../../dictation/model/dictation";
import { AttachmentChip } from "../../sessions/ui/AttachmentChip";
import type { Attachment } from "../../sessions/model/session";
import {
  attachmentsFromFiles,
  filesFromClipboard,
  mergeAttachments,
  pickAttachments,
  revokeAttachment,
} from "../../sessions/model/attachments";
import {
  getComposerDraft,
  getComposerQuotes,
  setComposerDraft,
  setComposerQuotes,
} from "../../sessions/model/draftCache";
import {
  addComposerQuote,
  withComposerQuotes,
  type ComposerQuote,
} from "../../sessions/model/composerQuotes";
import { QuoteChip } from "../../sessions/ui/QuoteChip";
import { useFileDrop } from "../../sessions/hooks/useFileDrop";
import { resizeComposer } from "../../sessions/model/composerResize";
import { useComposerAutocorrect } from "../../settings/model/displayPrefs";
import {
  consumeQuoteRequest,
  type QuoteRequest,
} from "../../sessions/model/quoteDraft";

// Loaded on first open, with its emoji data, so neither weighs on startup.
const EmojiPickerPopover = lazy(() =>
  import("../../../shared/ui/EmojiPicker").then((module) => ({
    default: module.EmojiPickerPopover,
  })),
);

const DROP_STATE = { current: { attachmentsSupported: true, remote: false } };

/** Tallest the field grows before it scrolls, in px. */
const MAX_HEIGHT = 160;
/** The quiet buttons under the input: a glyph that gains a backdrop on hover. */
const TOOL_BUTTON =
  "grid size-6.5 shrink-0 place-items-center rounded-md text-content/55 transition-colors hover:bg-content/8 hover:text-content disabled:opacity-50";
const INPUT_COLUMNS = "grid-cols-[1.625rem_minmax(0,1fr)_1.625rem]";
const FIELD_CLASSES =
  "scrollbar-none block min-w-0 w-full resize-none bg-transparent py-1 text-[13px] leading-4.5 text-content outline-none placeholder:text-content/35";

type Props = {
  sessionId: string;
  name: string;
  enabled?: boolean;
  focusToken?: number;
  quoteRequest?: QuoteRequest;
  onQuoteRequestConsumed?: (id: number) => void;
  onDraftChange?: (text: string) => void;
  /** Returns false when the message wasn't taken, so the draft stays. */
  onSubmit: (
    text: string,
    attachments: Attachment[],
  ) => boolean | void | Promise<boolean | void>;
  onFocus?: () => void;
  /**
   * Sits under the input on the right, like the model and its effort, with
   * attach across from it. Without any, attach and send share the field's
   * line, as in the floating chat.
   */
  controls?: ReactNode;
  /** Sits beside attach and emoji, on the left under the input. */
  tools?: ReactNode;
  /**
   * One line, nothing under it: attach and emoji open the field, these sit
   * after the text beside the mic. A group's chat, whose agents come with
   * their own models, has no model controls to show.
   */
  fieldTools?: ReactNode;
  /** Offers the mic; off where something else holds it, like a call. */
  dictation?: boolean;
  /** Square corners and tighter padding, for the floating chat. */
  square?: boolean;
  /**
   * Replaces "Message <name>" where the name is long, like a group's title.
   * The field's label keeps the full name for screen readers.
   */
  placeholder?: string;
};

/**
 * A messaging app's input for the Mono: attach, type, send, whether or not
 * it is mid-reply; Escape stops a reply. Files dropped anywhere on the chat
 * attach here. Attach sits under the input, with any `controls` (the model
 * and its effort) across from it, or beside the field when there are none;
 * permissions and checkout live in the details panel.
 */
export function MonoComposer({
  sessionId,
  name,
  enabled = true,
  focusToken,
  quoteRequest,
  onQuoteRequestConsumed,
  onDraftChange,
  onSubmit,
  onFocus,
  controls,
  tools,
  fieldTools,
  dictation: dictates = true,
  square = false,
  placeholder,
}: Props) {
  const corner = square ? "rounded-none" : "rounded-md";
  const [text, setText] = useState(() => getComposerDraft(sessionId) ?? "");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  // Text added with "Add to chat", shown as chips and sent as quotes.
  const [quotes, setQuotes] = useState<ComposerQuote[]>(() =>
    getComposerQuotes(sessionId),
  );
  const quotesRef = useRef(quotes);
  quotesRef.current = quotes;
  useEffect(() => setComposerQuotes(sessionId, quotes), [sessionId, quotes]);
  const autocorrect = useComposerAutocorrect();
  const textRef = useRef(text);
  const attachmentsRef = useRef(attachments);
  const consumedQuote = useRef<number | null>(null);
  const readGeneration = useRef(0);
  const pendingReadsRef = useRef(0);
  const [pendingReads, setPendingReads] = useState(0);
  const [multiline, setMultiline] = useState(false);
  const field = useRef<HTMLTextAreaElement>(null);
  const inlineMeasure = useRef<HTMLTextAreaElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const [dropError, setDropError] = useState<string | null>(null);
  const submittingRef = useRef(false);
  const [submitting, setSubmitting] = useState(false);
  const ready =
    enabled &&
    !submitting &&
    pendingReads === 0 &&
    (text.trim().length > 0 || attachments.length > 0 || quotes.length > 0);
  const compact = fieldTools !== undefined;
  const inline = !controls && !compact;
  // Attachments sit above the field like a second line of text, so an inline
  // field moves above the buttons for them too.
  const stacked = inline && (multiline || attachments.length > 0);

  useLayoutEffect(() => {
    const el = field.current;
    if (!el) return;
    const measure = inlineMeasure.current;
    const fit = () => {
      if (!el.clientWidth) return;
      if (measure?.clientWidth) {
        // Always measure at the inline width. Expanding the field above the
        // buttons must not make it collapse again just because it is wider.
        const style = getComputedStyle(measure);
        const singleLineHeight =
          Number.parseFloat(style.lineHeight) +
          Number.parseFloat(style.paddingTop) +
          Number.parseFloat(style.paddingBottom);
        setMultiline(measure.scrollHeight > singleLineHeight + 1);
      }
      // Holds the input's height while measuring, as sessions do, so the
      // transcript above never grows for a moment and loses its bottom pin.
      resizeComposer(el, MAX_HEIGHT);
      el.style.overflowY = el.scrollHeight > MAX_HEIGHT ? "auto" : "hidden";
    };
    fit();
    // An empty field stays one line at any width, so once measured, resizing
    // the panes beside it need not measure it every frame.
    const sized = measure ?? el;
    if (!text && sized.clientWidth) return;
    let width = sized.clientWidth;
    const observer = new ResizeObserver(() => {
      // Only a new width can rewrap the text; growing taller is our own doing.
      if (sized.clientWidth === width) return;
      width = sized.clientWidth;
      fit();
    });
    observer.observe(sized);
    return () => observer.disconnect();
  }, [text, multiline, stacked]);

  useLayoutEffect(() => {
    if (focusToken != null) field.current?.focus();
  }, [focusToken]);

  const update = (next: string) => {
    textRef.current = next;
    setText(next);
    setComposerDraft(sessionId, next);
    onDraftChange?.(next);
  };
  // Dictated text goes in at the cursor.
  const dictation = useDictation((spoken) => {
    const el = field.current;
    const current = textRef.current;
    const { value, cursor } = insertDictated(
      current,
      el?.selectionStart ?? current.length,
      el?.selectionEnd ?? current.length,
      spoken,
    );
    update(value);
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(cursor, cursor);
    });
  });
  const emojiButton = useRef<HTMLButtonElement>(null);
  const [emojiOpen, setEmojiOpen] = useState(false);
  // Where the pick goes: the field's selection when the picker opened.
  const emojiSelection = useRef<[number, number] | null>(null);
  const toggleEmoji = () => {
    if (emojiOpen) return setEmojiOpen(false);
    const el = field.current;
    emojiSelection.current = el ? [el.selectionStart, el.selectionEnd] : null;
    setEmojiOpen(true);
  };
  const insertEmoji = (emoji: string) => {
    const current = textRef.current;
    const [start, end] = emojiSelection.current ?? [
      current.length,
      current.length,
    ];
    update(current.slice(0, start) + emoji + current.slice(end));
    setEmojiOpen(false);
    const caret = start + emoji.length;
    requestAnimationFrame(() => {
      field.current?.focus();
      field.current?.setSelectionRange(caret, caret);
    });
  };
  useLayoutEffect(() => {
    // A quote waits as a chip; only plain inserts go into the text.
    if (quoteRequest && quoteRequest.mode !== "plain") {
      if (consumedQuote.current !== quoteRequest.id) {
        consumedQuote.current = quoteRequest.id;
        const added = quoteRequest.text;
        setQuotes((current) =>
          addComposerQuote(current, added, crypto.randomUUID()),
        );
        if (enabled) field.current?.focus();
      }
      onQuoteRequestConsumed?.(quoteRequest.id);
      return;
    }
    const next = consumeQuoteRequest(
      textRef.current,
      consumedQuote.current,
      quoteRequest,
    );
    consumedQuote.current = next.consumedId;
    if (next.changed) {
      update(next.draft);
      if (enabled) field.current?.focus();
    }
    if (next.consumedId != null) onQuoteRequestConsumed?.(next.consumedId);
  }, [quoteRequest, onQuoteRequestConsumed, enabled]);
  useLayoutEffect(
    () => () => {
      readGeneration.current++;
      for (const file of attachmentsRef.current) revokeAttachment(file);
    },
    [],
  );
  const add = (incoming: Attachment[]) => {
    if (incoming.length === 0) return;
    const next = mergeAttachments(attachmentsRef.current, incoming);
    attachmentsRef.current = next;
    setAttachments(next);
  };
  const readAttachments = useCallback(
    (read: () => Promise<Attachment[]>, dropped = false) => {
      if (submittingRef.current) return;
      const generation = readGeneration.current;
      pendingReadsRef.current++;
      setPendingReads(pendingReadsRef.current);
      setDropError(null);
      Promise.resolve()
        .then(read)
        .then((incoming) => {
          if (generation !== readGeneration.current) {
            incoming.forEach(revokeAttachment);
            return;
          }
          if (incoming.length === 0) {
            if (dropped)
              setDropError(
                "Nothing to attach from that drop — the file may have been moved, renamed, or deleted.",
              );
            return;
          }
          add(incoming);
        })
        .catch((reason: unknown) => {
          if (generation === readGeneration.current)
            setDropError(
              reason instanceof Error ? reason.message : String(reason),
            );
        })
        .finally(() => {
          pendingReadsRef.current--;
          if (generation === readGeneration.current)
            setPendingReads(pendingReadsRef.current);
        });
    },
    [],
  );
  const readDropped = useCallback(
    (read: () => Promise<Attachment[]>) => {
      readAttachments(read, true);
    },
    [readAttachments],
  );
  const fileDrag = useFileDrop({
    anchor: box,
    enabled: enabled && !submitting,
    state: DROP_STATE,
    read: readDropped,
    onError: setDropError,
  });
  const send = () => {
    if (!enabled || submittingRef.current || pendingReadsRef.current > 0)
      return;
    const sentQuotes = quotesRef.current;
    const message = withComposerQuotes(textRef.current.trim(), sentQuotes);
    const draft = textRef.current;
    const files = attachmentsRef.current;
    if (!message && !files.length) return;
    const generation = readGeneration.current;
    const clear = (accepted: boolean | void) => {
      if (accepted === false) return;
      if (generation !== readGeneration.current) {
        if (getComposerDraft(sessionId) === draft)
          setComposerDraft(sessionId, "");
        return;
      }
      update("");
      attachmentsRef.current = [];
      setAttachments([]);
      setQuotes((current) =>
        current.filter((quote) => !sentQuotes.includes(quote)),
      );
      setDropError(null);
    };
    const result = onSubmit(message, files);
    if (result instanceof Promise) {
      submittingRef.current = true;
      setSubmitting(true);
      void result
        .then(clear)
        .catch((reason: unknown) => {
          if (generation === readGeneration.current)
            setDropError(
              reason instanceof Error ? reason.message : String(reason),
            );
        })
        .finally(() => {
          submittingRef.current = false;
          if (generation === readGeneration.current) setSubmitting(false);
        });
    } else clear(result);
  };

  // `tool` sizes the plus to match the emoji face beside it: its cross fills
  // 14 of the glyph's 24 units against the face's 21, so it is drawn larger,
  // with a thinner stroke so both lines render about 1px wide.
  const attachButton = (className: string, tool = false) => (
    <button
      type="button"
      title="Attach files"
      aria-label="Attach files"
      disabled={!enabled || submitting}
      onClick={() => readAttachments(pickAttachments)}
      className={className}
    >
      <Plus
        className={tool ? "size-4.25" : "size-3.5"}
        strokeWidth={tool ? 1.5 : 1.75}
      />
    </button>
  );
  const input = (className: string) => (
    <textarea
      ref={field}
      rows={1}
      aria-label={`Message ${name}`}
      placeholder={placeholder ?? `Message ${name}`}
      spellCheck={autocorrect}
      autoCorrect={autocorrect ? "on" : "off"}
      disabled={!enabled || submitting}
      value={text}
      onFocus={onFocus}
      onChange={(event) => update(event.target.value)}
      onKeyDown={(event) => {
        if (
          event.key !== "Enter" ||
          event.shiftKey ||
          isImeComposition(event.nativeEvent)
        ) {
          return;
        }
        event.preventDefault();
        send();
      }}
      onPaste={(event) => {
        const files = filesFromClipboard(event.clipboardData);
        if (files.length === 0) return;
        event.preventDefault();
        readAttachments(() => attachmentsFromFiles(files));
      }}
      className={`${FIELD_CLASSES} ${className}`}
    />
  );
  const sendButton = (className: string) => (
    <button
      type="submit"
      aria-label="Send"
      title={pendingReads ? "Reading attachments…" : "Send"}
      disabled={!ready}
      className={`primary-action grid size-6.5 place-items-center ${corner} transition-[background-color,color,transform] duration-150 active:scale-90 disabled:cursor-default ${className}`}
    >
      <ArrowUp className="size-3.5" strokeWidth={2} />
    </button>
  );

  const emojiControl = (
    <>
      <button
        ref={emojiButton}
        type="button"
        title="Emoji"
        aria-label="Add emoji"
        aria-expanded={emojiOpen}
        disabled={!enabled || submitting}
        onClick={toggleEmoji}
        className={`${TOOL_BUTTON} ${emojiOpen ? "bg-content/8 text-content" : ""}`}
      >
        <SmilePlus className="size-3.25" strokeWidth={2} />
      </button>
      {emojiOpen ? (
        <Suspense fallback={null}>
          <EmojiPickerPopover
            anchor={emojiButton}
            onPick={insertEmoji}
            onDismiss={() => {
              setEmojiOpen(false);
              field.current?.focus();
            }}
          />
        </Suspense>
      ) : null}
    </>
  );

  return (
    <form
      className={`mx-auto w-full max-w-4xl shrink-0 font-sans ${square ? "p-0.5 pt-0" : "p-1.5 pt-0"}`}
      onSubmit={(event) => {
        event.preventDefault();
        send();
      }}
    >
      <div
        ref={box}
        className={`agent-chat-composer relative ${square ? "rounded-none" : "rounded-lg"} border bg-content/3 backdrop-blur-sm ${fileDrag ? "border-accent/60" : "border-content/10 has-focus:border-content/20"}`}
      >
        {fileDrag ? (
          <div
            className={`pointer-events-none absolute inset-0 z-20 grid place-items-center ${square ? "rounded-none" : "rounded-lg"} bg-accent/8 text-[12px] text-content/70`}
          >
            Drop files to attach
          </div>
        ) : null}
        {dropError ? (
          <p role="alert" className="px-3 pt-2 text-xs text-red-400">
            {dropError}
          </p>
        ) : null}
        {attachments.length > 0 || quotes.length > 0 ? (
          <div className="flex flex-wrap gap-1.5 px-2.5 pt-2">
            {quotes.map((quote) => (
              <QuoteChip
                key={quote.id}
                text={quote.text}
                onRemove={() => {
                  if (submittingRef.current) return;
                  setQuotes((current) =>
                    current.filter((entry) => entry.id !== quote.id),
                  );
                }}
              />
            ))}
            {attachments.map((file) => (
              <AttachmentChip
                key={file.id}
                attachment={file}
                onRemove={() => {
                  if (submittingRef.current) return;
                  revokeAttachment(file);
                  const next = attachmentsRef.current.filter(
                    (item) => item.id !== file.id,
                  );
                  attachmentsRef.current = next;
                  setAttachments(next);
                }}
              />
            ))}
          </div>
        ) : null}
        {inline ? (
          <div
            data-layout={stacked ? "multiline" : "inline"}
            className={`relative grid ${square ? "min-h-8" : "min-h-9"} ${INPUT_COLUMNS} items-center gap-x-1.5 p-1 ${stacked ? "gap-y-1" : ""}`}
          >
            <div
              aria-hidden="true"
              className={`pointer-events-none invisible absolute inset-x-1 top-1 grid ${INPUT_COLUMNS} gap-x-1.5`}
            >
              <textarea
                ref={inlineMeasure}
                tabIndex={-1}
                readOnly
                rows={1}
                value={text}
                className={`${FIELD_CLASSES} col-start-2 h-0 overflow-hidden`}
              />
            </div>
            {attachButton(
              `col-start-1 grid size-6.5 place-items-center ${corner} bg-content/8 text-content/55 hover:bg-content/12 hover:text-content ${stacked ? "row-start-2" : "row-start-1"}`,
            )}
            {input(
              `row-start-1 ${stacked ? "col-span-3 col-start-1 px-1.5" : "col-start-2"}`,
            )}
            {sendButton(
              `col-start-3 ${stacked ? "row-start-2" : "row-start-1"}`,
            )}
          </div>
        ) : compact ? (
          // Everything on the field's line; the buttons stay at its bottom
          // as the text grows, send last.
          <div className="flex items-end gap-0.5 p-1.25">
            <div className="flex shrink-0 items-center gap-0.5">
              {attachButton(TOOL_BUTTON, true)}
              {emojiControl}
            </div>
            {input("flex-1 self-center px-1.5")}
            <div className="flex shrink-0 items-center gap-1">
              {fieldTools}
              {enabled && dictates ? (
                <DictationButton dictation={dictation} />
              ) : null}
              {sendButton("shrink-0")}
            </div>
          </div>
        ) : (
          // Even padding centers send beside one line, and keeps it at the
          // bottom as the text grows.
          <div className="flex items-end gap-1.5 p-1.25">
            {input("flex-1 self-center px-1.5")}
            {enabled && dictates ? (
              <DictationButton dictation={dictation} />
            ) : null}
            {sendButton("shrink-0")}
          </div>
        )}
        <DictationBar dictation={dictation} />
      </div>
      {inline || compact ? null : (
        // The right inset matches the box's border and padding, so the last
        // control lines up under send. A container, so the tools and controls
        // passed in can compact themselves when the chat is narrow.
        <div className="@container flex min-w-0 items-center gap-0.5 pt-1 pl-1 pr-1.5">
          {attachButton(TOOL_BUTTON, true)}
          {emojiControl}
          {tools}
          <div className="ml-auto flex min-w-0 items-center gap-1">
            {controls}
          </div>
        </div>
      )}
    </form>
  );
}
