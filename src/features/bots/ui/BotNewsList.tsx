import { useEffect, useSyncExternalStore } from "react";
import { Bell } from "../../../shared/ui/icons";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import {
  botNewsSnapshot,
  markBotNewsSeen,
  subscribeBotNews,
} from "../model/botNews";

function when(at: number): string {
  const date = new Date(at);
  const today = new Date();
  return date.toDateString() === today.toDateString()
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** Everything the user's bots posted on their own, newest first. Reading
 * the feed marks it seen. */
export function BotNewsList({
  onOpenBot,
}: {
  onOpenBot: (monoId: string) => void;
}) {
  const items = useSyncExternalStore(subscribeBotNews, botNewsSnapshot);
  useEffect(() => {
    markBotNewsSeen();
  }, [items]);

  if (items.length === 0)
    return (
      <EmptyState icon={Bell}>
        When a bot&apos;s habit has something to tell you, like a digest, a
        budget alert or a finished report, it shows up here.
      </EmptyState>
    );

  return (
    <ul className="flex flex-col gap-px" data-bot-news>
      {items.map((item) => (
        <li key={item.id}>
          <button
            type="button"
            onClick={() => onOpenBot(item.monoId)}
            className="flex w-full flex-col gap-0.5 px-2 py-2 text-left hover:bg-content/6"
          >
            <span className="flex items-center gap-2 font-mono text-[11px] uppercase tracking-wide text-content/50">
              {!item.seen ? (
                <span aria-hidden className="size-1.5 shrink-0 bg-accent" />
              ) : null}
              <span className="min-w-0 truncate">
                {item.botName} · {item.source}
              </span>
              <span className="ml-auto shrink-0">{when(item.at)}</span>
            </span>
            <span className="line-clamp-4 whitespace-pre-line text-[13px] text-content/85">
              {item.text}
            </span>
          </button>
        </li>
      ))}
    </ul>
  );
}
