import { useState, type FormEvent } from "react";
import { LAYER } from "../../../shared/lib/layers";
import { BetaBadge } from "../../../shared/ui/BetaBadge";
import { Loader } from "../../../shared/ui/icons";
import { Modal } from "../../../shared/ui/Modal";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import type { RecentProject } from "../../projects/model/recents";
import { SearchableProjectPicker } from "../../projects/ui/SearchableProjectPicker";
import { workstreamBranchPrefix, workstreamSlug } from "../model/workstreams";

const NAME_MAX = 120;
const GOAL_MAX = 4000;

const CHECKOUTS = [
  {
    value: "worktree",
    label: "Its own worktree",
    keywords: "worktree branch separate",
  },
  { value: "project", label: "The project folder", keywords: "project here" },
];

export type NewWorkstream = {
  name: string;
  /** Sent to the orchestrator as its first message, when given. */
  goal: string;
  project: string;
  worktree: boolean;
};

/**
 * Names the workstream, gives its orchestrator the goal and the repository,
 * and picks where the orchestrator runs. Its agents always get worktrees of
 * their own.
 */
export function NewWorkstreamDialog({
  cwd,
  recents,
  onCreate,
  onClose,
}: {
  cwd: string;
  recents: RecentProject[];
  onCreate: (input: NewWorkstream) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const [project, setProject] = useState(cwd);
  const [worktree, setWorktree] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const hasProject = !!project && project !== "~";
  const ready = !!name.trim() && hasProject && !busy;

  const submit = (event: FormEvent) => {
    event.preventDefault();
    if (!ready) return;
    setBusy(true);
    setError(undefined);
    onCreate({ name: name.trim(), goal: goal.trim(), project, worktree })
      .then(onClose)
      .catch((reason: unknown) => {
        setError(String(reason));
        setBusy(false);
      });
  };

  const field =
    "rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none placeholder:text-content/35 focus:border-content/25 disabled:opacity-50";
  const label = "flex flex-col gap-1.5 text-[12px] text-content/70";
  const hint = "text-[11px] leading-snug text-content/45";
  return (
    <Modal
      title="New workstream"
      description="An orchestrator and its agents, working in parallel"
      size="md"
      onClose={() => {
        if (!busy) onClose();
      }}
    >
      <form className="flex flex-col gap-4 p-4" onSubmit={submit}>
        <label className={label}>
          <span className="flex items-center gap-2">
            Name
            <span className="ml-auto tabular-nums text-content/35">
              {name.length}/{NAME_MAX}
            </span>
          </span>
          <input
            autoFocus
            value={name}
            maxLength={NAME_MAX}
            disabled={busy}
            placeholder="Move the API to Hono"
            autoComplete="off"
            onChange={(event) => setName(event.target.value)}
            className={`h-9 ${field}`}
          />
          {name.trim() ? (
            <span className={hint}>
              Agents' branches:{" "}
              <span className="font-mono text-content/60">
                {workstreamBranchPrefix(workstreamSlug(name))}&lt;part&gt;
              </span>
            </span>
          ) : null}
        </label>
        <label className={label}>
          Goal
          <textarea
            value={goal}
            rows={4}
            maxLength={GOAL_MAX}
            disabled={busy}
            placeholder="What should be true when it is done, how to check it, and what to leave alone."
            onChange={(event) => setGoal(event.target.value)}
            className={`resize-none py-2 leading-relaxed ${field}`}
          />
          <span className={hint}>
            The orchestrator gets it as its first message and starts the plan.
            Leave it empty to explain it in the chat.
          </span>
        </label>
        <div className={label}>
          Repository
          <SearchableProjectPicker
            cwd={project}
            recents={recents}
            appearance="filled"
            className="w-full"
            buttonClassName="h-9 w-full justify-start rounded-md border border-content/10 bg-background-base px-2.5 text-[13px]"
            onSelectProject={setProject}
          />
        </div>
        <div className={label}>
          Orchestrator runs in
          <SearchableSelect
            label="Orchestrator runs in"
            value={worktree ? "worktree" : "project"}
            options={CHECKOUTS}
            searchable={false}
            disabled={busy}
            onChange={(value) => setWorktree(value === "worktree")}
            layer={LAYER.dialogPopover}
          />
          <span className={hint}>
            {worktree
              ? "Your open files stay untouched. Each agent gets a worktree of its own either way."
              : "The orchestrator shares the folder you work in. Each agent still gets a worktree of its own."}
          </span>
        </div>
        {error ? (
          <p role="alert" className="text-[12px] text-red-400">
            {error}
          </p>
        ) : null}
        <div className="flex items-center gap-2">
          <BetaBadge />
          <span className={hint}>
            Runs on this computer and stops when you quit MonoBOT.
          </span>
          <button
            type="button"
            disabled={busy}
            onClick={onClose}
            className="ml-auto rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 active:scale-[0.97]"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!ready}
            className="primary-action inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-[12px] font-medium active:scale-[0.97]"
          >
            {busy ? <Loader className="size-3.5 animate-spin" /> : null}
            Start workstream
          </button>
        </div>
      </form>
    </Modal>
  );
}
