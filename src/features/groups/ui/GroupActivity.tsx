import { useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { CircleAlert, Users } from "../../../shared/ui/icons";
import { TerminalSpinner } from "../../sessions/ui/TerminalSpinner";
import { formatLiveElapsed } from "../../sessions/model/liveAgents";
import {
  findMono,
  MONO_STATUS_LABEL,
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type Mono,
  type MonoState,
} from "../../monos/model/mono";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import { SubagentStack } from "../../sessions/ui/AgentTranscript";
import type { Session } from "../../sessions/model/session";
import { formatTokens } from "../../sessions/model/contextUsage";
import {
  delegationsSnapshot,
  subscribeDelegations,
} from "../../bots/model/delegations";
import {
  groupActivity,
  groupExchanges,
  type MemberActivity,
} from "../model/groupActivity";

function statusLine(state: MonoState): string {
  return state.status === "idle"
    ? "Idle"
    : (state.activity ?? MONO_STATUS_LABEL[state.status]);
}

function when(at: number): string {
  return new Date(at).toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
  });
}

/**
 * A group's Activity: each member on its own row, one under the other, with
 * what it is doing now and the subagents it launched, live; then what the
 * members asked each other. The Principal's row is the group chat it leads.
 */
export function GroupActivity({
  group,
  sessions,
  states,
  onOpenMember,
  onOpenFile,
}: {
  group: Mono;
  sessions: readonly Session[];
  states: ReadonlyMap<string, MonoState>;
  onOpenMember: (monoId: string) => void;
  onOpenFile?: (path: string) => void;
}) {
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const delegations = useSyncExternalStore(
    subscribeDelegations,
    delegationsSnapshot,
  );
  const rows = useMemo(
    () => groupActivity(group, sessions, states),
    // The roster is read through its snapshot.
    [group, sessions, states, snapshot],
  );
  const exchanges = useMemo(
    () => groupExchanges(group, delegations),
    [group, delegations],
  );
  const [now, setNow] = useState(() => Date.now());
  const ticking = rows.some((row) => row.startedAt != null);
  useEffect(() => {
    if (!ticking) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [ticking]);
  if (!rows.length)
    return (
      <EmptyState icon={Users}>
        Add members to the group; each one shows here with what it is doing and
        the subagents it runs.
      </EmptyState>
    );
  return (
    <div className="flex flex-col gap-3" data-group-activity>
      <ul className="flex flex-col gap-1">
        {rows.map((row) => (
          <MemberRow
            key={row.monoId}
            row={row}
            now={now}
            onOpen={() => onOpenMember(row.lead ? group.id : row.monoId)}
            onOpenFile={onOpenFile}
          />
        ))}
      </ul>
      {exchanges.length ? (
        <section className="flex flex-col gap-1 border-t border-stroke pt-3">
          <h4 className="px-2 text-[11px] text-content/45">Between members</h4>
          {exchanges.map((entry) => {
            const from = findMono(entry.from);
            const to = findMono(entry.to);
            if (!from || !to) return null;
            return (
              <div
                key={entry.id}
                className="flex flex-col gap-0.5 px-2 py-1.5 text-[12px]"
              >
                <span className="flex items-center gap-1.5 text-content/55">
                  <span className="font-medium text-content/80">
                    {monoLook(from).name}
                  </span>
                  asked
                  <span className="font-medium text-content/80">
                    {monoLook(to).name}
                  </span>
                  <span className="ml-auto tabular-nums text-content/40">
                    {when(entry.at)}
                  </span>
                </span>
                <span className="line-clamp-2 leading-relaxed text-content/70">
                  {entry.message}
                </span>
                {entry.reply ? (
                  <span className="line-clamp-3 border-l-2 border-content/10 pl-2 leading-relaxed text-content/60">
                    {entry.reply}
                  </span>
                ) : null}
              </div>
            );
          })}
        </section>
      ) : null}
    </div>
  );
}

/**
 * A member as a card, like the sidebar's working agents: what it is doing
 * with the spinner while it works, the call it waits on, how long the turn
 * has run, and the model and tokens it has spent in the group.
 */
function MemberRow({
  row,
  now,
  onOpen,
  onOpenFile,
}: {
  row: MemberActivity;
  now: number;
  onOpen: () => void;
  onOpenFile?: (path: string) => void;
}) {
  const { status } = row.state;
  const live = status !== "idle";
  const elapsed =
    live && row.startedAt != null ? formatLiveElapsed(row.startedAt, now) : "";
  const activity = status === "idle" ? row.role : statusLine(row.state);
  const usage = row.usage
    ? [
        row.usage.model,
        `${row.usage.turns} ${row.usage.turns === 1 ? "turn" : "turns"}`,
        row.usage.tokens ? `${formatTokens(row.usage.tokens)} tokens` : "",
      ]
        .filter(Boolean)
        .join(" · ")
    : "";
  return (
    <li className="flex flex-col" data-member-activity={row.monoId}>
      <button
        type="button"
        onClick={onOpen}
        title={[`Open ${row.name}`, activity, elapsed, usage]
          .filter(Boolean)
          .join("\n")}
        data-member-status={status}
        className={`flex w-full items-start gap-2.5 rounded-lg px-2.5 py-2 text-left ${
          live ? "bg-content/5 hover:bg-content/8" : "hover:bg-content/5"
        }`}
      >
        <MonoRailMascot
          name={row.mascot}
          color={row.color}
          status={status}
          className="mt-0.5 size-4 shrink-0"
        />
        <span className="flex min-w-0 flex-1 flex-col gap-1">
          <span className="flex items-center gap-2 text-[13px] leading-snug">
            <span className="min-w-0 truncate font-semibold text-content/90">
              {row.name}
            </span>
            {row.lead ? (
              <span className="shrink-0 rounded bg-accent/10 px-1.5 text-[11px] font-medium leading-4 text-accent">
                Leads
              </span>
            ) : null}
            {elapsed ? (
              <span className="ml-auto shrink-0 text-[11px] tabular-nums text-content/45">
                {elapsed}
              </span>
            ) : null}
          </span>
          <span
            className={`flex min-w-0 items-center gap-1.5 text-[12px] leading-tight ${
              status === "needs-you"
                ? "text-amber-400"
                : status === "working"
                  ? "text-content/65"
                  : "text-content/40"
            }`}
          >
            {status === "needs-you" ? (
              <CircleAlert className="size-3 shrink-0" strokeWidth={1.75} />
            ) : status === "working" ? (
              <TerminalSpinner className="inline-block w-3 select-none text-center text-[11px] leading-none text-accent" />
            ) : null}
            <span className="min-w-0 truncate">{activity}</span>
          </span>
          {usage || row.runs.length ? (
            <span className="flex min-w-0 items-center gap-1.5 text-[11px] leading-tight tabular-nums text-content/40">
              <span className="min-w-0 flex-1 truncate">{usage}</span>
              {row.runs.length ? (
                <span className="shrink-0">
                  {row.runs.length} subagent{row.runs.length === 1 ? "" : "s"}
                </span>
              ) : null}
            </span>
          ) : null}
        </span>
      </button>
      {row.runs.length ? (
        <div className="ml-6 border-l border-stroke pl-2">
          <SubagentStack
            blocks={row.runs}
            cwd={row.cwd}
            live={row.state.status === "working"}
            embedded
            onOpenFile={onOpenFile}
          />
        </div>
      ) : null}
    </li>
  );
}
