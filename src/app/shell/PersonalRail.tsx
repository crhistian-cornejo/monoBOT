import { useMemo, useSyncExternalStore } from "react";
import { Bell, Users } from "../../shared/ui/icons";
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
import { RailAction } from "./RailAction";

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

/** Code holds projects, Monos and workstreams; Personal holds the user's
 * bots. Each zone has its own rail below this switch. */
export function RailZoneSwitch({
  zone,
  onChange,
}: {
  zone: RailZone;
  onChange: (zone: RailZone) => void;
}) {
  const option = (value: RailZone, label: string) => (
    <button
      type="button"
      role="tab"
      aria-selected={zone === value}
      onClick={() => onChange(value)}
      className={`h-6 flex-1 font-mono text-[11px] uppercase tracking-wide ${
        zone === value
          ? "bg-content text-background-base"
          : "text-content/55 hover:bg-content/6 hover:text-content"
      }`}
    >
      {label}
    </button>
  );
  return (
    <div
      role="tablist"
      aria-label="Sidebar zone"
      className="mx-2 mb-2 flex shrink-0 border border-stroke"
    >
      {option("code", "Code")}
      {option("personal", "Personal")}
    </div>
  );
}

const IDLE: MonoState = { status: "idle" };

/** The Personal zone: the Principal bot on top, the news the bots posted,
 * then every personal bot. */
export function PersonalRail({ monos }: { monos: MonoRailProps }) {
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const news = useSyncExternalStore(subscribeBotNews, botNewsSnapshot);
  const newsUnseen = news.some((item) => !item.seen);
  const newsActive = !!monos.newsActive;
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
  // News opens on the Principal's overview, but only one row is the current
  // one: News while the feed shows, the Principal otherwise.
  const active = !!principal && monos.activeId === principal.id && !newsActive;

  return (
    <>
      <div className="flex shrink-0 flex-col gap-px px-2 pb-2">
        <button
          type="button"
          aria-current={active ? "true" : undefined}
          onClick={() =>
            principal
              ? monos.onOpen(principal.id)
              : monos.onCreateBot?.("principal")
          }
          // The same row as every bot's below, so it reads as one of them.
          className={`flex h-8 items-center gap-2 rounded-md px-2 text-left text-sm ${
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
            <span className="grid h-4 min-w-4 place-items-center bg-accent px-1 text-[10px] font-medium text-white">
              {unseen > 99 ? "99+" : unseen}
            </span>
          ) : null}
        </button>
        <RailAction
          label="News"
          icon={Bell}
          onClick={() => monos.onOpenNews?.()}
          active={newsActive}
          dot={newsUnseen}
          ariaLabel={newsUnseen ? "News, new posts" : "News"}
        />
      </div>
      <MonoRailSection {...monos} kind="bot" />
      <MonoRailSection {...monos} kind="group" />
    </>
  );
}
