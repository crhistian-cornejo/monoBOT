import type { ArtifactCard } from "../../artifacts/artifacts";

/**
 * The overview beside a bot's chat follows its turn: to the subagents while
 * they run, and to the documents it made once it is done, opening the final
 * report when there is one. What the user does in the panel during the turn
 * wins: then it stays where they put it.
 */
export type PanelMove =
  | { tab: "subagents" }
  | { tab: "documents"; document?: string };

export type TurnView = {
  busy: boolean;
  /** Subagent runs in the chat so far. */
  agentRuns: number;
  cards: readonly ArtifactCard[];
};

export type FollowState = {
  busy: boolean;
  agentRuns: number;
  /** The documents the chat had when the turn began. */
  startCards: ReadonlySet<string>;
  /** The user moved the panel during this turn. */
  touched: boolean;
};

const ids = (cards: readonly ArtifactCard[]) =>
  new Set(cards.map((card) => card.id));

/** The first look at a chat: nothing moves until a turn is seen to start. */
export function startFollow(view: TurnView): FollowState {
  return {
    busy: view.busy,
    agentRuns: view.agentRuns,
    startCards: ids(view.cards),
    touched: false,
  };
}

export function touchFollow(state: FollowState): FollowState {
  return state.touched ? state : { ...state, touched: true };
}

export function followTurn(
  state: FollowState,
  view: TurnView,
): { state: FollowState; move: PanelMove | null } {
  // A turn begins: remember what was there, and forget earlier touches.
  if (!state.busy && view.busy)
    return {
      state: {
        busy: true,
        agentRuns: view.agentRuns,
        startCards: ids(view.cards),
        touched: false,
      },
      move: null,
    };
  const next: FollowState = { ...state, busy: view.busy, agentRuns: view.agentRuns };
  if (view.busy) {
    const started = view.agentRuns > state.agentRuns;
    return {
      state: next,
      move: started && !state.touched ? { tab: "subagents" } : null,
    };
  }
  if (!state.busy) return { state: next, move: null };
  // The turn ended: what it made is new since it began.
  const made = view.cards.filter((card) => !state.startCards.has(card.id));
  const done: FollowState = { ...next, startCards: ids(view.cards) };
  if (!made.length || state.touched) return { state: done, move: null };
  const final = made.find((card) => card.final);
  return {
    state: done,
    move: final ? { tab: "documents", document: final.id } : { tab: "documents" },
  };
}
