import { describe, expect, it } from "vitest";
import { prGroup } from "./workstreamApi";
import {
  parseWorkstream,
  workstreamBranchPrefix,
  workstreamBrief,
  workstreamSlug,
} from "./workstreams";

const workstream = {
  project: "/repo",
  cwd: "/repo-worktrees/ws-migrate",
  slug: "migrate-to-react-19",
  folder: "/data/workstreams/abc",
  createdAt: 1,
};

describe("workstreams", () => {
  it("turns any name into a branch-safe slug", () => {
    expect(workstreamSlug("Migrar a React 19 — ¡ya!")).toBe("migrar-a-react-19-ya");
    expect(workstreamSlug("   ")).toBe("workstream");
    expect(workstreamSlug("x".repeat(80))).toHaveLength(40);
    expect(workstreamBranchPrefix("api")).toBe("ws/api/");
  });

  it("keeps only complete workstreams from storage", () => {
    expect(parseWorkstream(workstream)).toEqual(workstream);
    expect(parseWorkstream({ ...workstream, cwd: undefined })?.cwd).toBe("/repo");
    expect(parseWorkstream({ project: "/repo" })).toBeUndefined();
    expect(parseWorkstream(null)).toBeUndefined();
  });

  it("tells the orchestrator to coordinate, isolate agents and ship PRs", () => {
    const brief = workstreamBrief("Migrate", workstream);
    expect(brief).toContain("you do not write the code yourself");
    expect(brief).toContain('"workspaceMode":"worktree"');
    expect(brief).toContain("ws/migrate-to-react-19/<area>");
    expect(brief).toContain("gh pr create");
    expect(brief).toContain("/data/workstreams/abc");
  });

  it("groups pull requests the way the PRs tab shows them", () => {
    const pr = { number: 1, title: "", url: "", headRefName: "ws/a/b" };
    expect(prGroup({ ...pr, state: "OPEN", isDraft: false })).toBe("Open");
    expect(prGroup({ ...pr, state: "OPEN", isDraft: true })).toBe("Draft");
    expect(prGroup({ ...pr, state: "MERGED", isDraft: false })).toBe("Merged");
    expect(prGroup({ ...pr, state: "CLOSED", isDraft: true })).toBe("Closed");
  });
});
