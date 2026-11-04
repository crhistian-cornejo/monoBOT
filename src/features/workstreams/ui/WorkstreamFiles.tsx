import { useCallback, useEffect, useState } from "react";
import { File, FolderOpen, RefreshCw } from "../../../shared/ui/icons";
import { listDir, revealPath, type FsEntry } from "../../../platform/tauri/fs";
import { EmptyState } from "../../monos/ui/monoPanelParts";
import type { Workstream } from "../model/workstreams";

/** The shared folder: plans, notes, findings and deliverables. */
export function WorkstreamFiles({
  workstream,
  onOpenFile,
}: {
  workstream: Workstream;
  onOpenFile?: (path: string) => void;
}) {
  const [entries, setEntries] = useState<FsEntry[] | null>(null);
  const load = useCallback(() => {
    listDir(workstream.folder)
      .then((list) =>
        setEntries(
          list
            .filter((entry) => !entry.isDir)
            .sort((a, b) => a.name.localeCompare(b.name)),
        ),
      )
      .catch(() => setEntries([]));
  }, [workstream.folder]);
  useEffect(() => {
    load();
    const timer = window.setInterval(load, 15_000);
    return () => window.clearInterval(timer);
  }, [load]);

  const actions = (
    <div className="flex items-center gap-1">
      <button
        type="button"
        onClick={() => void revealPath(workstream.folder)}
        className="flex items-center gap-1 px-1.5 py-0.5 text-[11px] text-content/55 hover:bg-content/6 hover:text-content"
      >
        <FolderOpen className="size-3" strokeWidth={1.75} />
        Show folder
      </button>
      <button
        type="button"
        onClick={load}
        className="ml-auto flex items-center gap-1 px-1.5 py-0.5 text-[11px] text-content/55 hover:bg-content/6 hover:text-content"
      >
        <RefreshCw className="size-3" strokeWidth={1.75} />
        Refresh
      </button>
    </div>
  );

  if (entries && entries.length === 0)
    return (
      <div className="flex flex-col gap-2">
        {actions}
        <EmptyState icon={File}>
          The orchestrator and its agents keep the plan, notes and findings
          here. Code ships as pull requests.
        </EmptyState>
      </div>
    );

  return (
    <div className="flex flex-col gap-1" data-workstream-files>
      {actions}
      <ul className="flex flex-col gap-px">
        {(entries ?? []).map((entry) => (
          <li key={entry.path}>
            <button
              type="button"
              onClick={() => onOpenFile?.(entry.path)}
              className="flex w-full items-center gap-2 px-2 py-1.5 text-left text-[13px] hover:bg-content/6"
            >
              <File
                className="size-3.5 shrink-0 text-content/50"
                strokeWidth={1.75}
              />
              <span className="min-w-0 truncate">{entry.name}</span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}
