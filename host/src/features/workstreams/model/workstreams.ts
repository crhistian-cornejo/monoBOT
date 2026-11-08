/**
 * A workstream takes on work that would keep a team busy for days: the user
 * gives a goal to an orchestrator, which splits it into areas, starts one
 * agent per area in its own worktree, keeps them coordinated, and returns the
 * work as pull requests. The orchestrator is a Mono of kind "workstream": its
 * chat is the lead session, the sessions it starts are its agents.
 */
export type Workstream = {
  /** The repository the agents work on. */
  project: string;
  /** Where the orchestrator runs: the project, or a worktree of its own. */
  cwd: string;
  /** Names the agents' branches: ws/<slug>/<area>. */
  slug: string;
  /** The shared folder for plans, notes and findings; code ships as PRs. */
  folder: string;
  createdAt: number;
};

export function parseWorkstream(value: unknown): Workstream | undefined {
  if (!value || typeof value !== "object") return undefined;
  const entry = value as Record<string, unknown>;
  const text = (key: string) =>
    typeof entry[key] === "string" && entry[key]
      ? (entry[key] as string)
      : undefined;
  const project = text("project");
  const cwd = text("cwd") ?? project;
  const slug = text("slug");
  const folder = text("folder");
  if (!project || !cwd || !slug || !folder) return undefined;
  return {
    project,
    cwd,
    slug,
    folder,
    createdAt: typeof entry.createdAt === "number" ? entry.createdAt : 0,
  };
}

/** A branch-safe name from the workstream's name. */
export function workstreamSlug(name: string): string {
  const slug = name
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 40)
    .replace(/-+$/g, "");
  return slug || "workstream";
}

export function workstreamBranchPrefix(slug: string): string {
  return `ws/${slug}/`;
}

/** What the orchestrator is told on top of the Mono rules it already has. */
export function workstreamBrief(name: string, workstream: Workstream): string {
  const prefix = workstreamBranchPrefix(workstream.slug);
  return `<workstream>
You are the orchestrator of the workstream "${name}" on the repository at ${workstream.project}. You coordinate; you do not write the code yourself.
- Wait for the user's goal. Agree with them on the outcome, how success is judged, the constraints, how the work is verified, and which decisions they want to make. Write the plan to ${workstream.folder}/plan.md.
- Split the work into independent areas and start one agent per area with app sessions.start, "workspaceMode":"worktree", in this project. Give each agent its instructions, the files it owns, the checks it must pass, and its branch: ${prefix}<area>. Launch independent areas in the same turn so they run in parallel.
- Each agent finishes by renaming its worktree branch to its ${prefix}<area> branch, committing, pushing, and opening a draft pull request with gh pr create, then reports the PR URL to you.
- Agents report to you. Ask the user only when a decision is theirs alone. Reuse agents that already know the code (app sessions.send) instead of starting new ones for follow-ups.
- Plans, notes, findings and deliverables that are not code go in the shared folder ${workstream.folder}; code ships only as pull requests.
- When the user sets a standing rule (how PRs are opened, what to check), follow it for every agent from then on.
</workstream>`;
}

const INTRO_KEY = "monocode.workstreams.introSeen";

export function workstreamIntroSeen(): boolean {
  try {
    return localStorage.getItem(INTRO_KEY) === "1";
  } catch {
    return false;
  }
}

export function markWorkstreamIntroSeen() {
  try {
    localStorage.setItem(INTRO_KEY, "1");
  } catch {
    // shown again next time
  }
}
