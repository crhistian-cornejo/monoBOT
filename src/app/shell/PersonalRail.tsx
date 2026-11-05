import { useMemo, useSyncExternalStore } from "react";
import { Bot, Code, Users } from "../../shared/ui/icons";
import { MonoRailMascot } from "../../features/monos/ui/MonoRailMascot";
import {
  listMonos,
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type MonoState,
} from "../../features/monos/model/mono";
import { MonoRailSection, type MonoRailProps } from "./MonoRailSection";
import {
  botNewsSnapshot,
  subscribeBotNews,
} from "../../features/bots/model/botNews";

export type RailZone = "code" | "personal";

const ZONE_KEY = "monocode.railZone";

export function loadRailZone(): RailZone {
  try {
    return localStorage.getItem(ZONE_KEY) === "personal" ? "personal" : "code";
  } catch {
    return "code";
  }
}

export function saveRailZone(zone: RailZone) {
  try {
    localStorage.setItem(ZONE_KEY, zone);
  } catch {
    // private mode: the switch still works for this window
  }
}

/** Code holds projects, Monos and workstreams; Agents (the "personal" zone)
 * holds the user's bots and groups. Each zone has its own rail below this
 * switch. */
export function RailZoneSwitch({
  zone,
  onChange,
  className = "mx-2 mb-2",
}: {
  zone: RailZone;
  onChange: (zone: RailZone) => void;
  className?: string;
}) {
  // The segmented control Settings uses, so both layouts read the same.
  const option = (value: RailZone, label: string, Icon: typeof Code) => (
    <button
      type="button"
      role="tab"
      aria-selected={zone === value}
      onClick={() => onChange(value)}
      className={`flex min-w-0 items-center justify-center gap-1.5 rounded-sm px-2.5 py-1 ${
        zone === value
          ? "bg-selection text-content"
          : "text-content/50 hover:text-content"
      }`}
    >
      <Icon className="size-3.5 shrink-0" strokeWidth={1.75} />
      <span className="truncate">{label}</span>
    </button>
  );
  return (
    <div
      role="tablist"
      aria-label="Sidebar zone"
      className={`grid shrink-0 grid-cols-2 gap-0.5 rounded-md border border-content/10 p-0.5 text-[12px] ${className}`}
    >
      {option("code", "Code", Code)}
      {/* The bots and groups: the user's agents. */}
      {option("personal", "Agents", Bot)}
    </div>
  );
}

const IDLE: MonoState = { status: "idle" };

/** A hairline between the rail's groups of items. */
export function RailDivider() {
  return <div aria-hidden className="mx-3 mb-2 h-px shrink-0 bg-stroke" />;
}

/** The Personal zone: the Principal bot pinned on top, then the bots and
 * the groups, each under its own label. The bots' news lives on the
 * Principal's overview; a dot here says there is something unread. */
export function PersonalRail({
  monos,
  inset = false,
}: {
  monos: MonoRailProps;
  /** The focused layout's sidebar: its section labels and spacing. */
  inset?: boolean;
}) {
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const news = useSyncExternalStore(subscribeBotNews, botNewsSnapshot);
  const newsUnseen = news.some((item) => !item.seen);
  const principal = useMemo(
    () =>
      listMonos().find(
        (mono) => mono.kind === "bot" && mono.botRole === "principal",
      ),
    [snapshot],
  );
  const look = principal ? monoLook(principal) : undefined;
  const state = principal ? (monos.states.get(principal.id) ?? IDLE) : IDLE;
  const unseen = principal ? (monos.unseenCounts?.get(principal.id) ?? 0) : 0;
  const active = !!principal && monos.activeId === principal.id;

  return (
    <>
      <div
        className="flex shrink-0 flex-col gap-px px-2 pb-2"
        data-rail-principal
      >
        <button
          type="button"
          aria-current={active ? "true" : undefined}
          onClick={() =>
            principal
              ? monos.onOpen(principal.id)
              : monos.onCreateBot?.("principal")
          }
          // The same row as every bot's below, so it reads as one of them.
          className={`flex h-8 items-center gap-2 rounded-md px-2 text-left ${
            inset ? "text-[13px]" : "text-sm"
          } ${
            active
              ? "bg-selection-strong text-content"
              : "opacity-65 hover:bg-content/6 hover:opacity-100"
          }`}
        >
          {look ? (
            <MonoRailMascot
              name={look.mascot}
              color={look.color}
              status={state.status}
            />
          ) : (
            <Users className="size-4 text-content/60" strokeWidth={1.75} />
          )}
          <span className="min-w-0 flex-1 truncate font-medium">
            {look?.name ?? "Principal"}
          </span>
          {unseen ? (
            <span className="grid h-4 min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] font-medium tabular-nums text-white">
              {unseen > 99 ? "99+" : unseen}
            </span>
          ) : newsUnseen ? (
            <span
              aria-label="Unread news"
              title="Unread news"
              className="size-1.5 shrink-0 rounded-full bg-accent"
            />
          ) : null}
        </button>
      </div>
      <RailDivider />
      <MonoRailSection {...monos} kind="bot" inset={inset} />
      <RailDivider />
      <MonoRailSection {...monos} kind="group" inset={inset} />
    </>
  );
}
