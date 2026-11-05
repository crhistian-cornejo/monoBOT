import { describe, expect, it } from "vitest";
import type { ArtifactCard } from "../../artifacts/artifacts";
import { followTurn, startFollow, touchFollow, type TurnView } from "./panelFollow";

const paper: ArtifactCard = { id: "paper-a", kind: "document", title: "Clark" };
const report: ArtifactCard = { id: "r", kind: "document", title: "Informe", final: true };
const view = (busy: boolean, agentRuns = 0, cards: ArtifactCard[] = []): TurnView => ({
  busy,
  agentRuns,
  cards,
});

describe("panel follow", () => {
  it("goes to the subagents while they run, then opens the final report", () => {
    let state = startFollow(view(false));
    let step = followTurn(state, view(true));
    expect(step.move).toBeNull();
    step = followTurn(step.state, view(true, 1));
    expect(step.move).toEqual({ tab: "subagents" });
    step = followTurn(step.state, view(true, 1, [paper]));
    expect(step.move).toBeNull();
    step = followTurn(step.state, view(false, 1, [paper, report]));
    expect(step.move).toEqual({ tab: "documents", document: "r" });
    state = step.state;
    expect(followTurn(state, view(false, 1, [paper, report])).move).toBeNull();
  });

  it("shows the documents when the turn made some but no final report", () => {
    const step = followTurn(followTurn(startFollow(view(false)), view(true)).state, view(false, 0, [paper]));
    expect(step.move).toEqual({ tab: "documents" });
  });

  it("stays put when the turn made nothing new", () => {
    const start = startFollow(view(false, 0, [paper]));
    const step = followTurn(followTurn(start, view(true, 0, [paper])).state, view(false, 0, [paper]));
    expect(step.move).toBeNull();
  });

  it("does not move on the first look at a chat already working", () => {
    const step = followTurn(startFollow(view(true, 2)), view(false, 2, [report]));
    expect(step.move).toEqual({ tab: "documents", document: "r" });
    const fresh = startFollow(view(true, 2, [report]));
    expect(followTurn(fresh, view(false, 2, [report])).move).toBeNull();
  });

  it("leaves the panel where the user put it during the turn", () => {
    let state = followTurn(startFollow(view(false)), view(true)).state;
    state = touchFollow(state);
    expect(followTurn(state, view(true, 1)).move).toBeNull();
    expect(followTurn(state, view(false, 0, [report])).move).toBeNull();
    // The next turn follows again.
    const next = followTurn(followTurn(state, view(false)).state, view(true));
    expect(followTurn(next.state, view(true, 1)).move).toEqual({ tab: "subagents" });
  });
});
