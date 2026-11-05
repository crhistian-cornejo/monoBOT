import {
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { Bell, MessageSquare } from "../../../shared/ui/icons";
import { CollapsibleBody } from "../../../shared/ui/CollapsibleBody";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
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
import {
  findMono,
  monosSnapshot,
  subscribeMonos,
  type AgentNeedingYou,
  type MonoState,
} from "../../monos/model/mono";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import { useMonoCardPreview } from "../../monos/ui/MonoCardPreview";

/** Where a report was posted: its bot's chat and the message there. */
export type NewsPlace = { sessionId: string; blockId: string };
type OpenBot = (monoId: string, at?: NewsPlace) => void;

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
  waiting = [],
}: {
  /** Opens the bot's chat, at the message the report was posted as. */
  onOpenBot: OpenBot;
  /** Notes are turned on, so a report can be saved as one. */
  notes?: boolean;
  /** Agents waiting on an approval or an answer right now. */
  waiting?: readonly AgentNeedingYou[];
}) {
  const items = useSyncExternalStore(subscribeBotNews, botNewsSnapshot);
  useEffect(() => {
    markBotNewsSeen();
  }, [items]);

  if (items.length === 0 && waiting.length === 0)
    return (
      <EmptyState icon={Bell}>
        When an agent waits for your approval, or a bot&apos;s habit has
        something to tell you, like a digest, a budget alert or a finished
        report, it shows up here.
      </EmptyState>
    );

  return (
    <div className="flex flex-col gap-3">
      {waiting.length ? <NeedsYouList waiting={waiting} onOpenBot={onOpenBot} /> : null}
      {items.length ? <NewsItems items={items} notes={notes} onOpenBot={onOpenBot} /> : null}
    </div>
  );
}

/** Agents waiting on the user, live: each opens its chat to answer. */
function NeedsYouList({
  waiting,
  onOpenBot,
}: {
  waiting: readonly AgentNeedingYou[];
  onOpenBot: OpenBot;
}) {
  return (
    <section aria-label="Needs you" data-needs-you>
      <h3 className="px-2 pb-1 text-[11px] font-medium uppercase tracking-wide text-accent">
        Needs you
      </h3>
      <ul className="flex flex-col gap-px">
        {waiting.map((agent) => (
          <li key={agent.monoId}>
            <button
              type="button"
              onClick={() => onOpenBot(agent.monoId)}
              title={`Open ${agent.name}`}
              className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left hover:bg-content/5"
            >
              <span
                aria-hidden
                className="size-1.5 shrink-0 rounded-full bg-accent"
              />
              <span className="shrink-0 text-[13px] font-medium text-content/85">
                {agent.name}
              </span>
              <span className="min-w-0 truncate text-[12px] text-content/55">
                {agent.activity}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The bots that posted, with how many reports each, for the filters. */
function senders(items: ReturnType<typeof botNewsSnapshot>) {
  const counts = new Map<string, { name: string; count: number }>();
  for (const item of items) {
    const known = counts.get(item.monoId);
    counts.set(item.monoId, {
      name: known?.name ?? item.botName,
      count: (known?.count ?? 0) + 1,
    });
  }
  return [...counts].map(([monoId, entry]) => ({ monoId, ...entry }));
}

/** A bot's face in the feed: its mascot, or a quiet dot once it is gone. */
function BotFace({ monoId, size }: { monoId: string; size: string }) {
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const mono = findMono(monoId);
  return mono ? (
    <MonoRailMascot
      name={mono.mascot}
      color={mono.color}
      className={`${size} shrink-0`}
    />
  ) : (
    <span
      aria-hidden
      className={`${size} shrink-0 rounded-full bg-content/10`}
    />
  );
}

const NO_STATES: ReadonlyMap<string, MonoState> = new Map<string, MonoState>();

function NewsItems({
  items,
  notes,
  onOpenBot,
}: {
  items: ReturnType<typeof botNewsSnapshot>;
  notes: boolean;
  onOpenBot: OpenBot;
}) {
  const [only, setOnly] = useState<string>();
  const bots = useMemo(() => senders(items), [items]);
  // A filter whose bot has no news left shows everything again.
  const filter = bots.some((bot) => bot.monoId === only) ? only : undefined;
  const shown = filter
    ? items.filter((item) => item.monoId === filter)
    : items;
  // A bot's face and name show its card on hover, as in the rail.
  const preview = useMonoCardPreview(NO_STATES);
  return (
    <section aria-label="News" className="flex flex-col gap-2">
      {bots.length > 1 ? (
        <div
          role="toolbar"
          aria-label="Show news from"
          className="flex flex-wrap gap-1"
        >
          <FilterChip
            active={!filter}
            onClick={() => setOnly(undefined)}
            label={`All · ${items.length}`}
          />
          {bots.map((bot) => (
            <FilterChip
              key={bot.monoId}
              active={filter === bot.monoId}
              onClick={() =>
                setOnly(filter === bot.monoId ? undefined : bot.monoId)
              }
              label={`${bot.name} · ${bot.count}`}
              face={<BotFace monoId={bot.monoId} size="size-3.5" />}
            />
          ))}
        </div>
      ) : null}
      <ul className="flex flex-col gap-2" data-bot-news>
        {shown.map((item) => {
          const at =
            item.sessionId && item.blockId
              ? { sessionId: item.sessionId, blockId: item.blockId }
              : undefined;
          const needsYou = item.status === "needs_attention";
          return (
            <li
              key={item.id}
              className="group rounded-lg border border-content/8 bg-content/[0.02] px-3 pt-2.5"
            >
              <div className="flex items-center gap-2 text-[12px] text-content/50">
                <button
                  type="button"
                  onClick={() => onOpenBot(item.monoId)}
                  aria-label={`Open ${item.botName}`}
                  {...preview.bind(item.monoId)}
                  className="flex min-w-0 items-center gap-2 rounded-sm text-left outline-none hover:text-content/80 focus-visible:ring-1 focus-visible:ring-accent/50"
                >
                  <span className="relative grid shrink-0 place-items-center">
                    <BotFace monoId={item.monoId} size="size-4.5" />
                    {!item.seen ? (
                      <span
                        aria-label="Unread"
                        className="absolute -left-1 -top-0.5 size-1.5 rounded-full bg-accent ring-1 ring-background-base"
                      />
                    ) : null}
                  </span>
                  <span className="shrink-0 text-[13px] font-medium text-content/85">
                    {item.botName}
                  </span>
                  <span className="min-w-0 truncate">{item.source}</span>
                </button>
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
              </div>
              {/* The report reads as it does in the bot's chat, clamped. */}
              <div className="pl-6.5 pt-1.5 text-[13px]">
                <CollapsibleBody maxHeight={140}>
                  <AgentMarkdown
                    className="inbox-comment-md"
                    text={item.text}
                    streaming={false}
                    allowRemoteMedia
                  />
                </CollapsibleBody>
              </div>
              <div className="flex items-center gap-1 pb-1.5 pl-5 pt-0.5">
                {/* Where it was posted: the bot's chat, at that message. */}
                <button
                  type="button"
                  onClick={() => onOpenBot(item.monoId, at)}
                  className={`flex h-6 items-center gap-1.5 rounded-md px-1.5 text-[12px] hover:bg-content/8 ${
                    needsYou
                      ? "text-accent"
                      : "text-content/55 hover:text-content"
                  }`}
                >
                  <MessageSquare className="size-3.5" strokeWidth={1.75} />
                  {needsYou ? "Reply in chat" : "Open in chat"}
                </button>
                {/* The chat's own actions, shown on hover as under a reply. */}
                <div className="flex items-center gap-1 opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
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
              </div>
            </li>
          );
        })}
      </ul>
      {preview.card}
    </section>
  );
}

function FilterChip({
  active,
  label,
  face,
  onClick,
}: {
  active: boolean;
  label: string;
  face?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      className={`flex h-6 items-center gap-1.5 rounded-full border text-[12px] ${
        face ? "pl-1 pr-2.5" : "px-2.5"
      } ${
        active
          ? "border-content/20 bg-content/8 text-content"
          : "border-content/8 text-content/55 hover:bg-content/5 hover:text-content"
      }`}
    >
      {face}
      {label}
    </button>
  );
}
