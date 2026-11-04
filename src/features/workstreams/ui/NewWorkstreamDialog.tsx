import { useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { SearchableProjectPicker } from "../../projects/ui/SearchableProjectPicker";
import type { RecentProject } from "../../projects/model/recents";

const NAME_MAX = 120;

/**
 * Names the workstream and picks where its orchestrator runs. The agents it
 * starts work in their own worktrees of the same repository.
 */
export function NewWorkstreamDialog({
  cwd,
  recents,
  onCreate,
  onClose,
}: {
  cwd: string;
  recents: RecentProject[];
  onCreate: (input: {
    name: string;
    project: string;
    worktree: boolean;
  }) => Promise<void>;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [project, setProject] = useState(cwd);
  const [worktree, setWorktree] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const ready = !!name.trim() && !!project && project !== "~" && !busy;

  const submit = () => {
    if (!ready) return;
    setBusy(true);
    setError(undefined);
    onCreate({ name: name.trim(), project, worktree })
      .then(onClose)
      .catch((reason: unknown) => {
        setError(String(reason));
        setBusy(false);
      });
  };

  const label = "font-mono text-[11px] uppercase tracking-wide text-content/55";
  return (
    <Modal title="New workstream" onClose={onClose} size="md">
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <label className="flex flex-col gap-1.5">
          <span className={label}>Workstream name *</span>
          <input
            autoFocus
            value={name}
            maxLength={NAME_MAX}
            placeholder="What should this workstream accomplish?"
            onChange={(event) => setName(event.target.value)}
            className="h-9 border border-stroke bg-transparent px-2.5 text-[13px] outline-none focus:border-accent"
          />
          <span className="self-end text-[11px] tabular-nums text-content/40">
            {name.length}/{NAME_MAX}
          </span>
        </label>
        <div className="flex flex-col gap-1.5">
          <span className={label}>Orchestrator machine *</span>
          <div className="flex h-9 items-center border border-stroke px-2.5 text-[13px]">
            Local
            <span className="ml-auto text-xs text-content/45">
              Stops when you quit MonoBOT
            </span>
          </div>
        </div>
        <div className="flex flex-col gap-1.5">
          <span className={label}>Working directory</span>
          <SearchableProjectPicker
            cwd={project}
            recents={recents}
            appearance="filled"
            className="w-full"
            buttonClassName="h-9 w-full justify-start border border-stroke px-2.5 text-[13px]"
            onSelectProject={setProject}
          />
          <span className="text-xs text-content/45">
            The orchestrator starts here; its agents get their own worktrees of
            this repository.
          </span>
        </div>
        <label className="flex items-center gap-2 text-[13px]">
          <input
            type="checkbox"
            checked={worktree}
            onChange={(event) => setWorktree(event.target.checked)}
          />
          Run the orchestrator in its own worktree
        </label>
        {error ? (
          <p role="alert" className="text-xs text-red-400">
            {error}
          </p>
        ) : null}
        <div className="flex items-center justify-end gap-2">
          <button
            type="button"
            onClick={onClose}
            className="h-8 border border-stroke px-3 text-xs hover:bg-content/5"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!ready}
            className="h-8 bg-accent px-3 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy ? "Starting…" : "Start"}
          </button>
        </div>
      </form>
    </Modal>
  );
}
