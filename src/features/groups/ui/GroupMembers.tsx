import { useMemo, useState, useSyncExternalStore } from "react";
import { Plus, Trash2, Users } from "../../../shared/ui/icons";
import { MonoRailMascot } from "../../monos/ui/MonoRailMascot";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import {
  listMonos,
  monoLook,
  monosSnapshot,
  subscribeMonos,
  type Mono,
} from "../../monos/model/mono";
import { GROUP_MAX_MEMBERS } from "../model/group";
import { memberRole } from "../model/groupBrief";

/** Agents a group can take in: everyone but other groups. */
export function groupCandidates(): Mono[] {
  return listMonos().filter((mono) => mono.kind !== "group");
}

/** The Principal leads every group: it is always in, and cannot leave. */
const leads = (mono: Mono) => mono.botRole === "principal";

function LeadTag() {
  return (
    <span
      className="shrink-0 rounded bg-accent/10 px-1.5 text-[11px] font-medium leading-4 text-accent"
      title="The Principal leads every group and is always in it."
    >
      Leads
    </span>
  );
}

function MemberRow({ mono, action }: { mono: Mono; action: React.ReactNode }) {
  const look = monoLook(mono);
  return (
    <div className="flex items-center gap-2 px-2 py-1.5">
      <MonoRailMascot name={look.mascot} color={look.color} status="idle" />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px] font-medium">
          {look.name}
        </span>
        <span className="block truncate text-xs text-content/50">
          {memberRole(mono)}
        </span>
      </span>
      {action}
    </div>
  );
}

/** Picks members: checkboxes over every agent, up to the group's limit. */
export function MemberPicker({
  selected,
  onChange,
}: {
  selected: readonly string[];
  onChange: (members: string[]) => void;
}) {
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const candidates = useMemo(groupCandidates, [snapshot]);
  if (candidates.length === 0)
    return (
      <p className="text-xs text-content/50">
        Create a bot or a Mono first; groups bring them together.
      </p>
    );
  const full = selected.length >= GROUP_MAX_MEMBERS;
  const ordered = [
    ...candidates.filter(leads),
    ...candidates.filter((mono) => !leads(mono)),
  ];
  return (
    <div className="flex max-h-64 flex-col overflow-y-auto rounded-md border border-content/10 bg-background-base">
      {ordered.map((mono) => {
        const on = selected.includes(mono.id);
        if (leads(mono))
          return (
            <MemberRow key={mono.id} mono={mono} action={<LeadTag />} />
          );
        return (
          <label key={mono.id} className="cursor-pointer hover:bg-content/4">
            <MemberRow
              mono={mono}
              action={
                <input
                  type="checkbox"
                  checked={on}
                  disabled={!on && full}
                  onChange={() =>
                    onChange(
                      on
                        ? selected.filter((id) => id !== mono.id)
                        : [...selected, mono.id],
                    )
                  }
                />
              }
            />
          </label>
        );
      })}
    </div>
  );
}

/** The group's overview tab: its members, and who can still join. */
export function GroupMembers({
  members,
  onChange,
}: {
  members: readonly string[];
  onChange: (members: string[]) => void;
}) {
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const [adding, setAdding] = useState(false);
  const all = useMemo(groupCandidates, [snapshot]);
  const current = members.flatMap(
    (id) => all.find((mono) => mono.id === id) ?? [],
  );
  const others = all.filter((mono) => !members.includes(mono.id));
  const full = members.length >= GROUP_MAX_MEMBERS;
  return (
    <div className="flex flex-col gap-1" data-group-members>
      {current.length ? (
        current.map((mono) => (
          <MemberRow
            key={mono.id}
            mono={mono}
            action={
              leads(mono) ? (
                <LeadTag />
              ) : (
                <button
                  type="button"
                  title="Remove from the group"
                  aria-label={`Remove ${monoLook(mono).name}`}
                  onClick={() =>
                    onChange(members.filter((id) => id !== mono.id))
                  }
                  className="grid size-6 place-items-center text-content/45 hover:bg-content/8 hover:text-content"
                >
                  <Trash2 className="size-3.5" strokeWidth={1.75} />
                </button>
              )
            }
          />
        ))
      ) : (
        <EmptyState icon={Users}>
          Add the agents this group brings together. Its chat leads them and
          brings each one in when its part comes up.
        </EmptyState>
      )}
      {others.length && !full ? (
        adding ? (
          <div className="mt-2 flex flex-col border-t border-stroke pt-2">
            {others.map((mono) => (
              <MemberRow
                key={mono.id}
                mono={mono}
                action={
                  <button
                    type="button"
                    onClick={() => onChange([...members, mono.id])}
                    className="px-2 py-0.5 text-xs text-accent hover:bg-content/6"
                  >
                    Add
                  </button>
                }
              />
            ))}
          </div>
        ) : (
          <button
            type="button"
            onClick={() => setAdding(true)}
            className="mt-1 flex items-center gap-1.5 px-2 py-1.5 text-left text-[13px] text-content/60 hover:bg-content/6 hover:text-content"
          >
            <Plus className="size-3.5" strokeWidth={1.75} />
            Add member
          </button>
        )
      ) : null}
    </div>
  );
}
