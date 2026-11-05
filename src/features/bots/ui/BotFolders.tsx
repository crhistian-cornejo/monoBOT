import { useState } from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { Folder, Plus, X } from "../../../shared/ui/icons";
import { pickFolders } from "../../../platform/tauri/fs";
import { BOT_FOLDERS_MAX, folderProblem, type BotFolder } from "../model/bots";

/**
 * The folders the user opened to a bot beyond its home, in its settings: one
 * row each, the same rows as a Mono's projects. Only the user adds them, so
 * nothing a bot reads can widen what it may touch.
 */
export function BotFolders({
  folders,
  canEdit,
  onChange,
  pickTitle = "Open folders to this bot",
}: {
  folders: readonly BotFolder[];
  /** The bot may change files at all; each folder then opts in. */
  canEdit: boolean;
  onChange: (folders: BotFolder[]) => void;
  /** The system picker's title. */
  pickTitle?: string;
}) {
  const [problem, setProblem] = useState<string>();
  const add = async () => {
    setProblem(undefined);
    const picked = await pickFolders(pickTitle);
    const next = [...folders];
    for (const path of picked) {
      const why = folderProblem(path, false);
      if (why) {
        setProblem(`${prettyCwd(path)}: ${why}`);
        continue;
      }
      if (!next.some((folder) => folder.path === path))
        next.push({ path, edit: false });
    }
    if (next.length > BOT_FOLDERS_MAX)
      setProblem(`At most ${BOT_FOLDERS_MAX} folders`);
    onChange(next.slice(0, BOT_FOLDERS_MAX));
  };
  return (
    <div className="flex min-w-0 flex-col divide-y divide-stroke">
      {folders.map((folder) => {
        const editProblem = folderProblem(folder.path, true);
        return (
          <div
            key={folder.path}
            className="group flex min-h-12 min-w-0 items-center gap-3 px-3 py-2"
          >
            <span className="grid size-8 shrink-0 place-items-center rounded-lg bg-content/5">
              <Folder className="size-4 text-content/60" strokeWidth={1.75} />
            </span>
            <span
              className="min-w-0 flex-1 truncate text-[13px] leading-5 text-content/90"
              title={folder.path}
            >
              {prettyCwd(folder.path)}
            </span>
            {canEdit && !editProblem ? (
              <label className="flex shrink-0 items-center gap-1.5 text-[12px] text-content/55">
                <input
                  type="checkbox"
                  checked={folder.edit}
                  onChange={(event) =>
                    onChange(
                      folders.map((entry) =>
                        entry.path === folder.path
                          ? { ...entry, edit: event.target.checked }
                          : entry,
                      ),
                    )
                  }
                />
                Can change
              </label>
            ) : (
              <span
                className="shrink-0 text-[12px] text-content/40"
                title={editProblem}
              >
                Read only
              </span>
            )}
            <button
              type="button"
              aria-label={`Remove ${prettyCwd(folder.path)}`}
              title="Remove"
              onClick={() =>
                onChange(folders.filter((entry) => entry.path !== folder.path))
              }
              className="grid size-6 shrink-0 place-items-center rounded-md text-content/45 opacity-0 hover:bg-content/8 hover:text-content focus-visible:opacity-100 group-hover:opacity-100"
            >
              <X className="size-3.5" strokeWidth={1.75} />
            </button>
          </div>
        );
      })}
      <button
        type="button"
        onClick={() => void add()}
        className="flex h-11 w-full items-center gap-3 px-3 text-left text-[13px] text-content/55 first:rounded-t-xl last:rounded-b-xl hover:bg-content/4 hover:text-content"
      >
        <span className="grid size-8 shrink-0 place-items-center rounded-lg border border-dashed border-content/15">
          <Plus className="size-3.5 shrink-0" strokeWidth={1.75} />
        </span>
        Add folder
      </button>
      {problem ? (
        <p role="alert" className="px-3 py-2 text-[12px] text-red-400">
          {problem}
        </p>
      ) : null}
    </div>
  );
}
