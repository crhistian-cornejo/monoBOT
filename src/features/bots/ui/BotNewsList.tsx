import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import { Bell, MessageSquare } from "../../../shared/ui/icons";
import { CollapsibleBody } from "../../../shared/ui/CollapsibleBody";
import { Popover } from "../../../shared/ui/Popover";
import { scrollParent } from "../../../shared/lib/scrollParent";
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
  newsPreview,
  subscribeBotNews,
  type BotNewsItem,
} from "../model/botNews";
import {
  findMono,
  monosSnapshot,
  subscribeMonos,
  type AgentNeedingYou,
} from "../../monos/model/mono";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";

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
    <MoreBelow>
      {waiting.length ? <NeedsYouList waiting={waiting} onOpenBot={onOpenBot} /> : null}
      {items.length ? <NewsItems items={items} notes={notes} onOpenBot={onOpenBot} /> : null}
    </MoreBelow>
  );
}

/** How close to the end the panel counts as at the bottom. */
const END_SLACK_PX = 24;

/**
 * The feed, its last visible reports blurring into the panel's bottom edge
 * while more wait below, so the panel reads as going on.
 */
function MoreBelow({ children }: { children: ReactNode }) {
  const root = useRef<HTMLDivElement>(null);
  const [more, setMore] = useState(false);
  useEffect(() => {
    const parent = scrollParent(root.current);
    if (!parent) return;
    const update = () =>
      setMore(
        parent.scrollHeight - parent.scrollTop - parent.clientHeight >
          END_SLACK_PX,
      );
    update();
    parent.addEventListener("scroll", update, { passive: true });
    const observer =
      typeof ResizeObserver === "undefined" ? null : new ResizeObserver(update);
    observer?.observe(parent);
    if (root.current) observer?.observe(root.current);
    return () => {
      parent.removeEventListener("scroll", update);
      observer?.disconnect();
    };
  }, []);
  return (
    <div ref={root} className="flex flex-col gap-3">
      {children}
      {/* Sticks to the panel's bottom edge, past the panel's own padding
          (px-2 py-2), and the fade rises from it. */}
      <div aria-hidden className="pointer-events-none sticky -bottom-2 -mt-3 h-0">
        <div
          data-more-below={more ? "" : undefined}
          className={`absolute -inset-x-2 bottom-0 h-14 bg-gradient-to-t from-background-base to-transparent backdrop-blur-[2px] transition-opacity duration-200 [mask-image:linear-gradient(to_top,#000_35%,transparent)] ${
            more ? "opacity-100" : "opacity-0"
          }`}
        />
      </div>
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

/** How long the pointer rests on a report before its summary shows. */
const SUMMARY_OPEN_MS = 250;
const SUMMARY_CLOSE_MS = 120;
const SUMMARY_MAX = 220;

/**
 * The hover summary of a report: `bind` goes on each report's header, `card`
 * renders the one under the pointer, which the pointer may cross into.
 */
function useNewsSummary() {
  const id = useId();
  const [shown, setShown] = useState<{
    item: BotNewsItem;
    anchor: HTMLElement;
  }>();
  const timer = useRef<number | null>(null);
  const cancel = useCallback(() => {
    if (timer.current != null) window.clearTimeout(timer.current);
    timer.current = null;
  }, []);
  const close = useCallback(() => {
    cancel();
    setShown(undefined);
  }, [cancel]);
  const later = useCallback(
    (run: () => void, ms: number) => {
      cancel();
      timer.current = window.setTimeout(run, ms);
    },
    [cancel],
  );
  useEffect(() => cancel, [cancel]);
  useEffect(() => {
    if (!shown) return;
    const onScroll = () => close();
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", close);
    };
  }, [shown, close]);
  const bind = (item: BotNewsItem) => ({
    "aria-describedby": shown?.item.id === item.id ? id : undefined,
    onPointerEnter: (event: React.PointerEvent<HTMLElement>) => {
      if (event.pointerType === "touch") return;
      const anchor = event.currentTarget;
      if (shown) {
        cancel();
        setShown({ item, anchor });
      } else later(() => setShown({ item, anchor }), SUMMARY_OPEN_MS);
    },
    onPointerLeave: () => later(close, SUMMARY_CLOSE_MS),
  });
  const card = shown ? (
    <Popover
      anchor={shown.anchor}
      side="left"
      align="start"
      gap={8}
      width={280}
      role="tooltip"
      id={id}
      data-news-summary={shown.item.id}
      onDismiss={close}
      onPointerEnter={cancel}
      onPointerLeave={() => later(close, SUMMARY_CLOSE_MS)}
      className="px-3 py-2.5"
    >
      <div className="flex items-center gap-1.5 text-[11px] text-content/55">
        <BotFace monoId={shown.item.monoId} size="size-3.5" />
        <span className="min-w-0 truncate">
          {shown.item.botName} · {shown.item.source}
        </span>
        <span className="ml-auto shrink-0 tabular-nums text-content/40">
          {when(shown.item.at)}
        </span>
      </div>
      <p className="mt-1.5 text-[12px] leading-[18px] text-content/80">
        {newsPreview(shown.item.text, SUMMARY_MAX) || "No text"}
      </p>
    </Popover>
  ) : null;
  return { bind, card };
}

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
  // A report's header shows a short summary of it on hover.
  const summary = useNewsSummary();
  return (
    <section aria-label="News" className="flex flex-col gap-2">
      {bots.length > 1 ? (
        // Faces and counts only, so every bot fits on one row.
        <div
          role="toolbar"
          aria-label="Show news from"
          className="flex flex-nowrap gap-1 overflow-x-auto [scrollbar-width:none]"
        >
          <FilterChip
            active={!filter}
            onClick={() => setOnly(undefined)}
            name="All"
            count={items.length}
          />
          {bots.map((bot) => (
            <FilterChip
              key={bot.monoId}
              active={filter === bot.monoId}
              onClick={() =>
                setOnly(filter === bot.monoId ? undefined : bot.monoId)
              }
              name={bot.name}
              count={bot.count}
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
                  {...summary.bind(item)}
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
      {summary.card}
    </section>
  );
}

/** A filter: the bot's face (or "All") and how many reports it posted. The
 * name stays in the tooltip and the label. */
function FilterChip({
  active,
  name,
  count,
  face,
  onClick,
}: {
  active: boolean;
  name: string;
  count: number;
  face?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      aria-label={`${name} · ${count}`}
      title={name}
      onClick={onClick}
      className={`flex h-6 shrink-0 items-center gap-1.5 rounded-full border text-[12px] tabular-nums ${
        face ? "pl-1 pr-2" : "px-2.5"
      } ${
        active
          ? "border-content/20 bg-content/8 text-content"
          : "border-content/8 text-content/55 hover:bg-content/5 hover:text-content"
      }`}
    >
      {face ?? <span>{name}</span>}
      <span className={face ? "" : "text-content/45"}>{count}</span>
    </button>
  );
}
