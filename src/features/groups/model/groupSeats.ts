import {
  findMono,
  listMonos,
  updateMono,
  type Mono,
} from "../../monos/model/mono";

/**
 * A member's seat in a group: its own conversation there. It is the same
 * agent (its soul, specialty, tools, model and permissions) answering the
 * group, with a history kept apart from its own chat, which stays free for
 * everything else. What it learns there goes to the group's memory.
 */
export type GroupSeat = { group: Mono; member: Mono };

/** The group and member whose seat this conversation is. */
export function seatOf(
  sessionId: string,
  roster: readonly Mono[] = listMonos(),
): GroupSeat | undefined {
  for (const group of roster) {
    const seats = group.group?.seats;
    if (!seats) continue;
    for (const [memberId, seatId] of Object.entries(seats)) {
      if (seatId !== sessionId) continue;
      const member = roster.find((mono) => mono.id === memberId);
      return member ? { group, member } : undefined;
    }
  }
  return undefined;
}

export function isGroupSeat(sessionId: string): boolean {
  return !!seatOf(sessionId);
}

/** The conversation a member has in a group, when it has spoken there. */
export function seatSessionId(
  groupId: string,
  memberId: string,
): string | undefined {
  return findMono(groupId)?.group?.seats?.[memberId];
}

/** Records the member's conversation in the group. */
export function saveSeat(
  groupId: string,
  memberId: string,
  sessionId: string,
) {
  updateMono(groupId, (mono) =>
    mono.group
      ? {
          ...mono,
          group: {
            ...mono.group,
            seats: { ...(mono.group.seats ?? {}), [memberId]: sessionId },
          },
        }
      : mono,
  );
}

/** Every seat conversation, for the lists that leave them out. */
export function seatSessionIds(
  roster: readonly Mono[] = listMonos(),
): Set<string> {
  return new Set(
    roster.flatMap((mono) => Object.values(mono.group?.seats ?? {})),
  );
}

/** What a member reads in its seat, ahead of the group's messages. */
export function seatBrief(groupName: string, memberName: string): string {
  return `<group_seat>
You are ${memberName}, in your own seat in the group "${groupName}". This conversation is yours only for this group: everything you write here shows in the group's chat under your name, and nothing here reaches your own chat with the user. Your soul, specialty, tools and model are the same as there. What you learn here about the project (decisions, preferences, people, files) goes to the group's memory: memory.add from this seat writes there; add "personal":true only for something about the user that holds beyond this group.
</group_seat>`;
}
