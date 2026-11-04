/**
 * A group brings agents together in one room: any of the user's Monos, bots
 * and workstreams. The group's own chat is its leader: it talks with the
 * user and brings members in with the app action group.ask, each member
 * answering from its own conversation.
 */
export type Group = {
  /** Mono ids of the members, in the order they were added. */
  members: string[];
  /** What the group is for, shared with every member. */
  goal?: string;
};

export const GROUP_MAX_MEMBERS = 8;

export function parseGroup(value: unknown): Group | undefined {
  if (!value || typeof value !== "object") return undefined;
  const entry = value as Record<string, unknown>;
  const members = Array.isArray(entry.members)
    ? [
        ...new Set(
          entry.members.filter(
            (id): id is string => typeof id === "string" && !!id,
          ),
        ),
      ].slice(0, GROUP_MAX_MEMBERS)
    : [];
  const goal =
    typeof entry.goal === "string" && entry.goal.trim()
      ? entry.goal.trim()
      : undefined;
  return { members, ...(goal ? { goal } : {}) };
}

/** What a member is told when the leader brings it in. */
export function groupMessage(
  groupName: string,
  goal: string | undefined,
  message: string,
): string {
  return `<group_message group="${groupName}">
${goal ? `The group is for: ${goal}\n` : ""}${message.trim()}
</group_message>
Answer as yourself with your own contribution, briefly unless asked for more. If you have nothing new to add, reply with exactly PASS.`;
}
