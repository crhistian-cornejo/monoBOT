import { invoke } from "@tauri-apps/api/core";
import { createWorktree } from "../../source-control/model/worktrees";
import {
  createWorkstreamMono,
  newWorkstreamId,
  type Mono,
} from "../../monos/model/mono";
import {
  workstreamBranchPrefix,
  workstreamSlug,
  type Workstream,
} from "./workstreams";

export type WorkstreamPr = {
  number: number;
  title: string;
  url: string;
  /** OPEN, CLOSED or MERGED, as GitHub reports it. */
  state: string;
  isDraft: boolean;
  headRefName: string;
};

/** The pull requests opened from the workstream's branches. */
export function listWorkstreamPrs(
  workstream: Workstream,
): Promise<WorkstreamPr[]> {
  return invoke<WorkstreamPr[]>("git_branch_prs", {
    cwd: workstream.project,
    prefix: workstreamBranchPrefix(workstream.slug),
  });
}

/** Factory's grouping: what still needs the user first. */
export function prGroup(
  pr: WorkstreamPr,
): "Open" | "Draft" | "Merged" | "Closed" {
  if (pr.state === "MERGED") return "Merged";
  if (pr.state === "CLOSED") return "Closed";
  return pr.isDraft ? "Draft" : "Open";
}

/**
 * Makes the workstream: its shared folder, its orchestrator's checkout (the
 * project itself, or a worktree of its own on ws/<slug>/orchestrator), and
 * the Mono that orchestrates it.
 */
export async function createWorkstream(input: {
  name: string;
  project: string;
  worktree: boolean;
}): Promise<Mono> {
  const name = input.name.trim().slice(0, 120);
  if (!name) throw new Error("Name the workstream");
  const id = newWorkstreamId();
  const slug = workstreamSlug(name);
  const folder = await invoke<string>("workstream_folder", { id });
  const cwd = input.worktree
    ? (
        await createWorktree(
          input.project,
          `${workstreamBranchPrefix(slug)}orchestrator`,
          "HEAD",
          false,
        )
      ).path
    : input.project;
  return createWorkstreamMono(id, name, {
    project: input.project,
    cwd,
    slug,
    folder,
    createdAt: Date.now(),
  });
}
