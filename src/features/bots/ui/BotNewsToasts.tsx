import { useEffect, useMemo, useRef, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../../../shared/lib/layers";
import { Bell, X } from "../../../shared/ui/icons";
import {
  listMonos,
  monoLook,
  monosSnapshot,
  subscribeMonos,
} from "../../monos/model/mono";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import {
  dismissNewsToast,
  newsPreview,
  newsToastsSnapshot,
  subscribeBotNews,
  type BotNewsItem,
} from "../model/botNews";

/** A card leaves on its own after this long, unless the pointer is on it. */
const TOAST_MS = 15_000;

/**
 * A bot's habit had something to say while the user was elsewhere: the
 * Principal brings it up as a card, so it is not only a dot in the rail.
 * Read opens the feed on the Principal; the bot's name opens its chat.
 */
export function BotNewsToasts({
  onRead,
  onOpenBot,
}: {
  onRead: () => void;
  onOpenBot: (monoId: string) => void;
}) {
  const items = useSyncExternalStore(subscribeBotNews, newsToastsSnapshot);
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const principal = useMemo(() => {
    const mono = listMonos().find(
      (entry) => entry.kind === "bot" && entry.botRole === "principal",
    );
    return mono ? monoLook(mono) : undefined;
  }, [snapshot]);
  if (items.length === 0) return null;

  return createPortal(
    <div
      aria-live="polite"
      style={{ zIndex: LAYER.toast }}
      className="pointer-events-none fixed right-3 bottom-3 flex w-[min(340px,calc(100vw-24px))] flex-col-reverse gap-2"
    >
      {items.map((item) => (
        <NewsToast
          key={item.id}
          item={item}
          principal={principal}
          onRead={() => {
            dismissNewsToast(item.id);
            onRead();
          }}
          onOpenBot={() => {
            dismissNewsToast(item.id);
            onOpenBot(item.monoId);
          }}
        />
      ))}
    </div>,
    document.body,
  );
}

function NewsToast({
  item,
  principal,
  onRead,
  onOpenBot,
}: {
  item: BotNewsItem;
  principal?: { name: string; mascot: string; color: string };
  onRead: () => void;
  onOpenBot: () => void;
}) {
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const start = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => dismissNewsToast(item.id), TOAST_MS);
  };
  const pause = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
  };
  useEffect(() => {
    start();
    return pause;
  }, [item.id]);

  return (
    <article
      role="status"
      onPointerEnter={pause}
      onPointerLeave={start}
      onFocus={pause}
      onBlur={start}
      className="pointer-events-auto overflow-hidden rounded-xl border border-content/15 bg-background-base/95 text-content shadow-xl backdrop-blur-xl"
    >
      <div className="flex items-center gap-2 px-3.5 pt-3">
        {principal ? (
          <MonoRailMascot name={principal.mascot} color={principal.color} />
        ) : (
          <Bell className="size-4 shrink-0 text-accent" strokeWidth={1.75} />
        )}
        <span className="min-w-0 flex-1 truncate text-[12px] text-content/55">
          <span className="font-medium text-content/80">
            {principal?.name ?? "News"}
          </span>
          {" · "}
          {item.botName} has news
        </span>
        <button
          type="button"
          aria-label="Dismiss"
          title="Dismiss"
          onClick={() => dismissNewsToast(item.id)}
          className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content"
        >
          <X className="size-3.5" strokeWidth={1.75} />
        </button>
      </div>
      <button
        type="button"
        onClick={onRead}
        className="block w-full px-3.5 pt-1 pb-2 text-left"
      >
        <span className="block truncate text-[13px] font-medium">
          {item.source}
        </span>
        <span className="mt-0.5 line-clamp-3 text-[12px] leading-relaxed text-content/65">
          {newsPreview(item.text, 280)}
        </span>
      </button>
      <div className="flex items-center gap-1.5 border-t border-stroke px-3.5 py-2 text-[12px]">
        <button
          type="button"
          className="primary-action shrink-0 rounded-md px-3 py-1.5 text-[12px] font-medium active:scale-[0.97]"
          onClick={onRead}
        >
          Read
        </button>
        <button
          type="button"
          onClick={onOpenBot}
          className="min-w-0 truncate rounded-md px-3 py-1.5 hover:bg-content/8"
        >
          Open {item.botName}
        </button>
      </div>
    </article>
  );
}
