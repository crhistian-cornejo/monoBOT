import type { Delegation } from "../../bots/model/delegations";
import {
  findMono,
  monoLook,
  monoState,
  type Mono,
  type MonoState,
} from "../../monos/model/mono";
import type { Block, Session } from "../../sessions/model/session";
import { memberRole } from "./groupBrief";

/** One row of a group's activity: a member, what it is doing, and the
 * subagents it launched in its latest turn. */
export type MemberActivity = {
  monoId: string;
  name: string;
  mascot: string;
  color: string;
  role: string;
  /** The Principal, whose work in the group is the group chat's. */
  lead: boolean;
  state: MonoState;
  /** Its latest turn's subagent runs, in the order they started. */
  runs: Block[];
  /** Where its chat runs, for the paths its subagents show. */
  cwd?: string;
};

type ActivitySession = Pick<
  Session,
  | "id"
  | "cwd"
  | "blocks"
  | "busy"
  | "pendingQuestion"
  | "worktreeRemoved"
  | "usageLimit"
>;

const IDLE: MonoState = { status: "idle" };

/** The subagent runs of the session's latest turn. */
export function latestRuns(blocks: readonly Block[]): Block[] {
  let start = 0;
  for (let i = blocks.length - 1; i >= 0; i--) {
    const block = blocks[i]!;
    if (block.role === "user" && !block.internal) {
      start = i + 1;
      break;
    }
  }
  return blocks.slice(start).filter((block) => !!block.agentRun);
}

/**
 * The group's members as its Activity panel lists them, one under the
 * other: the Principal first, read from the group chat it leads, then each
 * member from its own chat. A member whose chat is not open reads from the
 * rail's states.
 */
export function groupActivity(
  group: Mono,
  sessions: readonly ActivitySession[],
  states: ReadonlyMap<string, MonoState> = new Map(),
  find: (id: string) => Mono | undefined = findMono,
): MemberActivity[] {
  const members = (group.group?.members ?? []).flatMap((id) => find(id) ?? []);
  const sessionOf = (id?: string) =>
    id ? sessions.find((session) => session.id === id) : undefined;
  const rows = members.map((mono): MemberActivity => {
    const lead = mono.botRole === "principal";
    const session = sessionOf(lead ? group.sessionId : mono.sessionId);
    const look = monoLook(mono);
    return {
      monoId: mono.id,
      name: look.name,
      mascot: look.mascot,
      color: look.color,
      role: lead ? "Leads the group" : memberRole(mono),
      lead,
      state: session
        ? monoState(session)
        : lead
          ? (states.get(group.id) ?? IDLE)
          : (states.get(mono.id) ?? IDLE),
      runs: session ? latestRuns(session.blocks) : [],
      ...(session?.cwd ? { cwd: session.cwd } : {}),
    };
  });
  return [
    ...rows.filter((row) => row.lead),
    ...rows.filter((row) => !row.lead),
  ];
}

/** How far back the panel shows members asking each other. */
const EXCHANGE_WINDOW_MS = 24 * 60 * 60 * 1000;
const EXCHANGES_MAX = 8;

/** What the members asked each other lately, newest first. */
export function groupExchanges(
  group: Mono,
  delegations: readonly Delegation[],
  now = Date.now(),
): Delegation[] {
  const members = new Set(group.group?.members ?? []);
  return delegations
    .filter(
      (entry) =>
        members.has(entry.from) &&
        members.has(entry.to) &&
        now - entry.at < EXCHANGE_WINDOW_MS,
    )
    .slice(0, EXCHANGES_MAX);
}
