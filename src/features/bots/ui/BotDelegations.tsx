import { useSyncExternalStore } from "react";
import { Users } from "../../../shared/ui/icons";
import {
  findMono,
  MONO_STATUS_LABEL,
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type MonoState,
} from "../../monos/model/mono";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import {
  delegationsSnapshot,
  latestByAgent,
  subscribeDelegations,
} from "../model/delegations";

const IDLE: MonoState = { status: "idle" };

function when(at: number): string {
  const date = new Date(at);
  return date.toDateString() === new Date().toDateString()
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleDateString([], { month: "short", day: "numeric" });
}

/**
 * The Principal's agents are the user's bots: one row per bot it asked, with
 * what it asked last and what that bot is doing now. Opening a row shows the
 * bot's own chat, where the work and the answer are.
 */
export function BotDelegations({
  leadId,
  leadName,
  states,
  onOpenBot,
}: {
  leadId: string;
  leadName: string;
  states: ReadonlyMap<string, MonoState>;
  onOpenBot: (monoId: string) => void;
}) {
  const delegations = useSyncExternalStore(
    subscribeDelegations,
    delegationsSnapshot,
  );
  // Names and mascots follow the roster.
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const rows = latestByAgent(delegations, leadId).flatMap((entry) => {
    const mono = findMono(entry.to);
    return mono ? [{ entry, look: monoLook(mono) }] : [];
  });

  if (!rows.length)
    return (
      <EmptyState icon={Users}>
        When {leadName} hands part of a job to one of your bots, the bot shows
        up here with what it was asked and what it is doing.
      </EmptyState>
    );

  return (
    <ul className="flex flex-col gap-px" data-bot-delegations>
      {rows.map(({ entry, look }) => {
        const state = states.get(entry.to) ?? IDLE;
        const status =
          state.status === "idle"
            ? "Idle"
            : (state.activity ?? MONO_STATUS_LABEL[state.status]);
        return (
          <li key={entry.to}>
            <button
              type="button"
              onClick={() => onOpenBot(entry.to)}
              title={`Open ${look.name}`}
              className="flex w-full items-start gap-2.5 rounded-md px-2 py-2 text-left hover:bg-content/5"
            >
              <MonoRailMascot
                name={look.mascot}
                color={look.color}
                status={state.status}
                className="mt-0.5 size-4 shrink-0"
              />
              <span className="flex min-w-0 flex-1 flex-col gap-0.5">
                <span className="flex items-center gap-2 text-[12px]">
                  <span className="min-w-0 truncate font-medium text-content/85">
                    {look.name}
                  </span>
                  <span
                    className={`min-w-0 truncate ${
                      state.status === "needs-you"
                        ? "text-accent"
                        : state.status === "working"
                          ? "text-content/60"
                          : "text-content/40"
                    }`}
                  >
                    {status}
                  </span>
                  <span className="ml-auto shrink-0 tabular-nums text-content/40">
                    {when(entry.at)}
                  </span>
                </span>
                <span className="line-clamp-2 text-[13px] leading-relaxed text-content/70">
                  {entry.message}
                </span>
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}
