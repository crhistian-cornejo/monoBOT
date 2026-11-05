import { openUrl } from "@tauri-apps/plugin-opener";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Archive,
  ArrowLeft,
  Star,
} from "../../../shared/ui/icons";
import { openPathWithDefaultApp } from "../../../platform/tauri/fs";
import {
  loadAutoImages,
  loadInBatches,
  mailDocument,
  type MailAction,
  type MailMessage,
  type OpenedMail,
} from "../model/mail";
import { mailApi } from "../model/mailApi";
import { MailAttachments } from "./MailAttachments";
import { MailReply } from "./MailReply";

/** The most pictures on the web one message loads, and how many are fetched at once. */
const MAX_IMAGES = 40;
const IMAGES_AT_ONCE = 6;
/** Pictures that land close together redraw the frame once. */
const IMAGE_PAINT_MS = 250;

const ICON_BUTTON =
  "grid size-7 place-items-center rounded-md text-content/60 hover:bg-content/8 hover:text-content disabled:opacity-40";

/** The app's colors, resolved, for a frame that cannot read its variables. */
function frameColors() {
  const style = getComputedStyle(document.documentElement);
  const read = (name: string, fallback: string) =>
    style.getPropertyValue(name).trim() || fallback;
  return {
    text: read("--color-content", "#1f1f1f"),
    background: read("--color-background-base", "#ffffff"),
    link: read("--color-accent", "#2563eb"),
  };
}

/** Messages opened this session: going back to one shows it at once. */
const openedCache = new Map<string, OpenedMail>();
const OPENED_KEPT = 40;

function remember(opened: OpenedMail) {
  openedCache.delete(opened.id);
  openedCache.set(opened.id, opened);
  while (openedCache.size > OPENED_KEPT)
    openedCache.delete(openedCache.keys().next().value!);
}

export function MailReader({
  message,
  onBack,
  onAct,
  onInteresting,
  replyRequest,
}: {
  message: MailMessage;
  onBack: () => void;
  onAct: (action: MailAction) => void;
  onInteresting: (on: boolean) => void;
  /** Changes when the user presses R: opens the reply. */
  replyRequest: number;
}) {
  const [opened, setOpened] = useState<OpenedMail>();
  const [error, setError] = useState<string>();
  const [images, setImages] = useState<Record<string, string>>({});
  const [loadingImages, setLoadingImages] = useState(false);
  /** Bumped when the message changes, so a load still running for the last one stops. */
  const imageRun = useRef(0);
  const [replying, setReplying] = useState(false);
  const [notice, setNotice] = useState<string>();
  const held = message.triage === "held";

  useEffect(() => {
    let alive = true;
    const cached = openedCache.get(message.id);
    setOpened(cached);
    setError(undefined);
    setImages({});
    setReplying(false);
    setNotice(undefined);
    if (!cached)
      void mailApi
        .open(message.id)
        .then((result) => {
          remember(result);
          if (alive) setOpened(result);
        })
        .catch((failure) => alive && setError(String(failure)));
    return () => {
      alive = false;
    };
  }, [message.id]);

  useEffect(() => {
    if (replyRequest && !held) setReplying(true);
  }, [replyRequest, held]);

  /** Fetches the message's pictures on the web through Rust, a few at a time, painting them as they land. */
  const loadImages = useCallback(async (target: OpenedMail) => {
    const run = ++imageRun.current;
    const stale = () => imageRun.current !== run;
    const landed: Record<string, string> = {};
    let timer: number | undefined;
    const paint = () => {
      timer = undefined;
      if (!stale()) setImages((current) => ({ ...current, ...landed }));
    };
    setLoadingImages(true);
    await loadInBatches(
      target.remote.slice(0, MAX_IMAGES),
      IMAGES_AT_ONCE,
      (url) => mailApi.image(target.id, url),
      (url, data) => {
        landed[url] = data;
        timer ??= window.setTimeout(paint, IMAGE_PAINT_MS);
      },
      stale,
    );
    window.clearTimeout(timer);
    paint();
    if (!stale()) setLoadingImages(false);
  }, []);

  // A message's pictures load as it opens, unless the user turned that off or
  // the message is held back as a possible scam: those wait for a press.
  useEffect(() => {
    if (!opened || opened.id !== message.id || !opened.remote.length) return;
    if (held || !loadAutoImages()) return;
    void loadImages(opened);
    return () => {
      imageRun.current += 1;
      setLoadingImages(false);
    };
  }, [opened, message.id, held, loadImages]);

  return (
    <article className="flex min-h-0 flex-1 flex-col">
      <div className="flex h-10 shrink-0 items-center gap-1 border-b border-stroke px-2">
        <button type="button" title="Back to the list" aria-label="Back" onClick={onBack} className={ICON_BUTTON}>
          <ArrowLeft className="size-3.5" />
        </button>
        <div className="flex-1" />
        <button
          type="button"
          title={message.starred ? "Unstar (S)" : "Star (S)"}
          aria-label={message.starred ? "Unstar" : "Star"}
          onClick={() => onAct(message.starred ? "unstar" : "star")}
          className={`${ICON_BUTTON} ${message.starred ? "text-accent" : ""}`}
        >
          <Star className="size-3.5" />
        </button>
        <button
          type="button"
          title="Archive (E)"
          aria-label="Archive"
          disabled={message.folder === "archive"}
          onClick={() => onAct("archive")}
          className={ICON_BUTTON}
        >
          <Archive className="size-3.5" />
        </button>
        <button
          type="button"
          onClick={() => onInteresting(message.triage !== "priority")}
          className="rounded-md px-2.5 py-1 text-[12px] text-content/70 hover:bg-content/8"
        >
          {message.triage === "priority" ? "Not priority" : "Mark sender as priority"}
        </button>
        {held ? null : (
          <button
            type="button"
            onClick={() => setReplying(true)}
            className="rounded-md px-2.5 py-1 text-[12px] hover:bg-content/8"
          >
            Reply (R)
          </button>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto">
      {/* A column at least as tall as the pane, so the reply floats at its foot
          and stays there (sticky) while the message scrolls under it. */}
      <div className="flex min-h-full flex-col px-5 pt-4">
        <h2 className="text-[16px] font-medium">{message.subject || "(no subject)"}</h2>
        <p className="mt-1 text-[12px] text-content/60">
          {message.from} &lt;{message.address}&gt; ·{" "}
          {new Date(message.at).toLocaleString(undefined, {
            dateStyle: "medium",
            timeStyle: "short",
          })}
        </p>
        {held ? (
          <p className="mt-3 rounded-md border border-red-400/30 bg-red-400/8 px-3 py-2 text-[12px]">
            Held back{message.reason ? `: ${message.reason}` : ""}. It may be a
            scam, so it can't be answered from here. Don't open its links or
            files unless you trust the sender.
          </p>
        ) : null}

        {opened ? (
          <MailAttachments
            attachments={opened.attachments}
            onOpen={(file) =>
              void mailApi
                .attachment(message.id, file.index)
                .then(openPathWithDefaultApp)
                .catch((failure) => setNotice(String(failure)))
            }
          />
        ) : null}

        {opened?.remote.length && !Object.keys(images).length ? (
          loadingImages ? (
            <p className="mt-3 text-[12px] text-content/45">Loading pictures…</p>
          ) : (
            <button
              type="button"
              onClick={() => void loadImages(opened)}
              className="mt-3 self-start rounded-md px-2.5 py-1 text-[12px] text-content/60 hover:bg-content/8"
            >
              Show {opened.remote.length} picture{opened.remote.length === 1 ? "" : "s"} from the web
            </button>
          )
        ) : null}

        <div className="mt-4">
          {error ? (
            <p role="alert" className="text-xs text-red-400">{error}</p>
          ) : !opened ? (
            <p className="text-[13px] text-content/50">Opening…</p>
          ) : opened.html ? (
            <MailBody html={opened.html} images={images} />
          ) : (
            <div className="flex flex-col gap-3 text-[13px] leading-relaxed">
              {(opened.text.length ? opened.text : message.body).map((paragraph, index) => (
                <p key={index} className="whitespace-pre-wrap">{paragraph}</p>
              ))}
            </div>
          )}
        </div>
        {notice ? <p role="alert" className="mt-2 text-xs text-red-400">{notice}</p> : null}
        <div className="min-h-4 flex-1" />
        {replying ? (
          <div className="mail-reply-float sticky bottom-3 z-10 mx-auto mb-3 w-full max-w-[560px]">
            <MailReply
              message={message}
              opened={opened}
              onClose={() => setReplying(false)}
              onSent={() => {
                setReplying(false);
                setNotice(undefined);
              }}
            />
          </div>
        ) : null}
      </div>
      </div>
    </article>
  );
}

/** The message's HTML in a frame that runs no script and loads nothing from the web. */
function MailBody({ html, images }: { html: string; images: Record<string, string> }) {
  const frame = useRef<HTMLIFrameElement>(null);
  const [height, setHeight] = useState(200);
  const doc = useMemo(() => mailDocument(html, frameColors(), images), [html, images]);

  const onLoad = () => {
    const inner = frame.current?.contentDocument;
    if (!inner) return;
    setHeight(Math.min(inner.documentElement.scrollHeight + 8, 20_000));
    // Links open in the browser, and only web ones.
    inner.addEventListener("click", (event) => {
      const link = (event.target as Element | null)?.closest?.("a[href]");
      if (!link) return;
      event.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (/^https?:\/\//i.test(href)) void openUrl(href);
    });
  };

  return (
    <iframe
      ref={frame}
      title="Message"
      // Same origin only so the app can size it and catch its links; no scripts run in it.
      sandbox="allow-same-origin"
      srcDoc={doc}
      onLoad={onLoad}
      style={{ height }}
      className="w-full border-0"
    />
  );
}
