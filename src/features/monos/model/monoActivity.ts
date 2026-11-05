import type { Block, Session } from "../../sessions/model/session";
import { groupMonoChatTurns } from "../../sessions/model/transcriptActivity";

export type MonoActivitySelection = {
  sessionId: string;
  turnId: string;
  /** Retain an archived turn after the viewer returns to the live window. */
  blocks: Block[];
};

export function resolveMonoActivity(
  selection: MonoActivitySelection | null,
  session: Pick<Session, "id" | "blocks" | "busy"> | undefined,
) {
  if (!selection || selection.sessionId !== session?.id) return null;
  const turns = groupMonoChatTurns(session.blocks);
  const current = turns.find((turn) => turn[0].id === selection.turnId);
  return {
    turnId: selection.turnId,
    blocks: current ?? selection.blocks,
    live: !!session.busy && !!current && current === turns[turns.length - 1],
  };
}

/** Whether a turn did anything worth a trail: a tool, a subagent, a step. */
function hasWork(turn: Block[]): boolean {
  return turn.some((block) => block.role === "tool" || !!block.agentRun);
}

/**
 * What the Activity tab shows with no turn picked: the latest turn that did
 * some work, followed live while it runs, so the panel is never blank while
 * the agent is busy.
 */
export function latestMonoActivity(
  session: Pick<Session, "id" | "blocks" | "busy"> | undefined,
) {
  if (!session) return null;
  const turns = groupMonoChatTurns(session.blocks);
  const last = turns[turns.length - 1];
  const live = !!session.busy && !!last;
  const turn = live ? last : [...turns].reverse().find(hasWork);
  if (!turn) return null;
  return { turnId: turn[0].id, blocks: turn, live: live && turn === last };
}
