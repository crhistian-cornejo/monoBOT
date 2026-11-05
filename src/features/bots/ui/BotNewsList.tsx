import { useEffect, useSyncExternalStore } from "react";
import { Bell } from "../../../shared/ui/icons";
import { createNote } from "../../notes";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import {
  CopyTurnButton,
  SaveNoteButton,
} from "../../sessions/ui/MessageActions";
import {
  botNewsSnapshot,
  markBotNewsSeen,
  subscribeBotNews,
} from "../model/botNews";

const STATUS_LABEL = {
  done: "Done",
  blocked: "Blocked",
  needs_attention: "Needs you",
} as const;

const STATUS_STYLE = {
  done: "bg-emerald-500/10 text-emerald-500",
  blocked: "bg-red-400/10 text-red-400",
  needs_attention: "bg-accent/10 text-accent",
} as const;

function when(at: number): string {
  const date = new Date(at);
  const today = new Date();
  return date.toDateString() === today.toDateString()
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** Everything the user's bots posted on their own, newest first. Reading
 * the feed marks it seen. Each report can be copied or kept as a note, as a
 * reply in a chat can. */
export function BotNewsList({
  onOpenBot,
  notes = false,
}: {
  onOpenBot: (monoId: string) => void;
  /** Notes are turned on, so a report can be saved as one. */
  notes?: boolean;
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
        <li key={item.id} className="group rounded-md hover:bg-content/5">
          <button
            type="button"
            onClick={() => onOpenBot(item.monoId)}
            title={`Open ${item.botName}`}
            className="flex w-full flex-col gap-1 px-2 pt-2 text-left"
          >
            <span className="flex items-center gap-2 text-[12px] text-content/50">
              {!item.seen ? (
                <span
                  aria-label="Unread"
                  className="size-1.5 shrink-0 rounded-full bg-accent"
                />
              ) : null}
              <span className="min-w-0 truncate">
                <span className="font-medium text-content/75">
                  {item.botName}
                </span>
                {" · "}
                {item.source}
              </span>
              {item.status && item.status !== "progress" ? (
                <span
                  className={`shrink-0 rounded px-1.5 text-[11px] font-medium leading-4 ${STATUS_STYLE[item.status]}`}
                >
                  {STATUS_LABEL[item.status]}
                </span>
              ) : null}
              <span className="ml-auto shrink-0 tabular-nums text-content/40">
                {when(item.at)}
              </span>
            </span>
            <span className="line-clamp-4 whitespace-pre-line text-[13px] leading-relaxed text-content/85">
              {item.text}
            </span>
          </button>
          {/* The chat's own actions, shown on hover as under a reply. */}
          <div className="flex items-center gap-1 px-2 pb-1.5 pt-0.5 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
            <CopyTurnButton text={item.text} label="Copy report" />
            {notes ? (
              <SaveNoteButton
                text={item.text}
                onSave={async (text) => {
                  await createNote({
                    title: `${item.botName} · ${item.source}`,
                    body: text,
                  });
                }}
              />
            ) : null}
          </div>
        </li>
      ))}
    </ul>
  );
}
