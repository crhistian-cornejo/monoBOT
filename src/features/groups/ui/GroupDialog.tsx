import { useState, type ReactNode } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { prettyCwd } from "../../../shared/lib/paths";
import {
  Clock,
  File,
  Folder,
  Plus,
  Sparkles,
  StickyNote,
  X,
} from "../../../shared/ui/icons";
import { pickFiles } from "../../../platform/tauri/fs";
import { folderProblem, type BotFolder } from "../../bots/model/bots";
import { BotFolders } from "../../bots/ui/BotFolders";
import { GROUP_CONTEXT_MAX, GROUP_MAX_MEMBERS } from "../model/group";
import { withPrincipal } from "../../monos/model/mono";
import { MemberPicker } from "./GroupMembers";

/** What the dialog hands back: the group, and how its project starts. */
export type GroupDraft = {
  name: string;
  goal: string;
  members: string[];
  folders: BotFolder[];
  context: string[];
  /** New groups only: seeds the group chat's soul. */
  instructions?: string;
  /** New groups only: seeds its memory. */
  memory?: string;
  /** New groups only: open its Habits on a new scheduled task. */
  addTask?: boolean;
};

/** Where the group's own files are edited once it exists. */
export type GroupPage = "instructions" | "memory" | "habits";

const FIELD =
  "rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none placeholder:text-content/35 focus:border-content/25";
const LABEL = "flex flex-col gap-1.5 text-[12px] text-content/70";

/**
 * Names a group, says what it is for and picks who is in it; the Principal
 * is always in and leads. Like a project, it may also get folders opened to
 * its members (read only or read and change), instructions, context files,
 * memory and scheduled tasks, all optional. The same dialog edits a group:
 * its instructions, memory and tasks then open where they live.
 */
export function GroupDialog({
  initial,
  onSave,
  onOpenPage,
  onClose,
}: {
  /** The group being edited; a new group when left out. */
  initial?: Pick<
    GroupDraft,
    "name" | "goal" | "members" | "folders" | "context"
  >;
  onSave: (draft: GroupDraft) => void;
  /** Editing: opens the group's instructions, memory or habits. */
  onOpenPage?: (page: GroupPage) => void;
  onClose: () => void;
}) {
  const editing = !!initial;
  const [name, setName] = useState(initial?.name ?? "");
  const [goal, setGoal] = useState(initial?.goal ?? "");
  // The Principal is in from the start: it leads every group.
  const [members, setMembers] = useState<string[]>(() =>
    withPrincipal(initial?.members ?? []),
  );
  const [folders, setFolders] = useState<BotFolder[]>(initial?.folders ?? []);
  const [context, setContext] = useState<string[]>(initial?.context ?? []);
  const [instructions, setInstructions] = useState("");
  const [memory, setMemory] = useState("");
  const [addTask, setAddTask] = useState(false);
  const [open, setOpen] = useState<Set<string>>(
    () =>
      new Set([
        ...(folders.length ? ["folders"] : []),
        ...(context.length ? ["context"] : []),
      ]),
  );
  const [problem, setProblem] = useState<string>();
  const ready = !!name.trim();
  const show = (id: string) => setOpen((current) => new Set(current).add(id));
  const addContext = async () => {
    setProblem(undefined);
    const picked = (await pickFiles("Add context files")) ?? [];
    const next = [...context];
    for (const path of picked) {
      const why = folderProblem(path, false);
      if (why) setProblem(`${prettyCwd(path)}: ${why}`);
      else if (!next.includes(path)) next.push(path);
    }
    if (next.length > GROUP_CONTEXT_MAX)
      setProblem(`At most ${GROUP_CONTEXT_MAX} files`);
    setContext(next.slice(0, GROUP_CONTEXT_MAX));
    show("context");
  };
  return (
    <Modal
      title={editing ? "Group settings" : "New group"}
      description={
        editing
          ? "Its goal, members and what they may open"
          : "Agents that work on one goal together, led by your Principal"
      }
      onClose={onClose}
      size="md"
      fitViewport
    >
      <form
        className="flex max-h-[min(72dvh,680px)] min-h-0 flex-col"
        onSubmit={(event) => {
          event.preventDefault();
          if (!ready) return;
          onSave({
            name: name.trim(),
            goal: goal.trim(),
            members,
            folders,
            context,
            ...(editing
              ? {}
              : {
                  ...(instructions.trim()
                    ? { instructions: instructions.trim() }
                    : {}),
                  ...(memory.trim() ? { memory: memory.trim() } : {}),
                  ...(addTask ? { addTask } : {}),
                }),
          });
          onClose();
        }}
      >
        <div className="flex min-h-0 flex-col gap-4 overflow-y-auto p-4">
          <label className={LABEL}>
            Name
            <input
              autoFocus
              value={name}
              maxLength={60}
              placeholder="Morning briefing, Trip planning, Report review…"
              autoComplete="off"
              onChange={(event) => setName(event.target.value)}
              className={`h-9 ${FIELD}`}
            />
          </label>
          <label className={LABEL}>
            What it is for
            <textarea
              value={goal}
              rows={2}
              maxLength={500}
              placeholder="Shared with every member when the group brings them in."
              onChange={(event) => setGoal(event.target.value)}
              className={`resize-none py-2 leading-relaxed ${FIELD}`}
            />
          </label>
          <div className={LABEL}>
            <span className="flex items-center">
              Members
              <span className="ml-auto tabular-nums text-content/35">
                {members.length}/{GROUP_MAX_MEMBERS}
              </span>
            </span>
            <MemberPicker selected={members} onChange={setMembers} />
          </div>

          <div className="flex flex-col gap-1.5">
            <span className="text-[12px] text-content/70">
              Project <span className="text-content/40">· optional</span>
            </span>
            <div className="flex flex-col divide-y divide-stroke rounded-md border border-content/10 bg-background-base">
              <ProjectRow
                icon={Folder}
                title="Folders"
                note={
                  folders.length
                    ? `${folders.length} open to the members`
                    : "Open folders to the members, read only or to change"
                }
                action={open.has("folders") ? undefined : "Add"}
                onAction={() => show("folders")}
              >
                {open.has("folders") ? (
                  <>
                    <BotFolders
                      folders={folders}
                      canEdit
                      pickTitle="Open folders to this group"
                      onChange={setFolders}
                    />
                    <p className="px-3 pb-2 text-[11px] leading-4 text-content/45">
                      Members read these without asking. Tick “Can change” to
                      let them edit there; a bot edits only if its own tools
                      allow changing files, and nobody deletes. For anything
                      else, your Principal asks you first.
                    </p>
                  </>
                ) : null}
              </ProjectRow>
              <ProjectRow
                icon={Sparkles}
                title="Instructions"
                note="How the group works: tone, steps, what to check"
                action={
                  editing
                    ? "Edit"
                    : open.has("instructions")
                      ? undefined
                      : "Add"
                }
                onAction={() =>
                  editing ? onOpenPage?.("instructions") : show("instructions")
                }
              >
                {!editing && open.has("instructions") ? (
                  <textarea
                    autoFocus
                    value={instructions}
                    rows={4}
                    maxLength={8000}
                    placeholder="Answer in Spanish. Review the drawings before the reports…"
                    onChange={(event) => setInstructions(event.target.value)}
                    className={`mx-3 mb-3 resize-y py-2 leading-relaxed ${FIELD}`}
                  />
                ) : null}
              </ProjectRow>
              <ProjectRow
                icon={File}
                title="Context"
                note={
                  context.length
                    ? `${context.length} file${context.length === 1 ? "" : "s"} the members read`
                    : "Files the members read for background"
                }
                action="Add"
                onAction={() => void addContext()}
              >
                {open.has("context") && context.length ? (
                  <div className="flex flex-col pb-1">
                    {context.map((path) => (
                      <div
                        key={path}
                        className="group/file flex min-w-0 items-center gap-2 px-3 py-1"
                      >
                        <span
                          className="min-w-0 flex-1 truncate text-[12px] text-content/80"
                          title={path}
                        >
                          {prettyCwd(path)}
                        </span>
                        <button
                          type="button"
                          aria-label={`Remove ${prettyCwd(path)}`}
                          title="Remove"
                          onClick={() =>
                            setContext(
                              context.filter((entry) => entry !== path),
                            )
                          }
                          className="grid size-5 shrink-0 place-items-center rounded-md text-content/45 hover:bg-content/8 hover:text-content"
                        >
                          <X className="size-3" strokeWidth={1.75} />
                        </button>
                      </div>
                    ))}
                  </div>
                ) : null}
              </ProjectRow>
              <ProjectRow
                icon={StickyNote}
                title="Memory"
                note="What the group remembers between conversations"
                action={
                  editing ? "See" : open.has("memory") ? undefined : "Add"
                }
                onAction={() =>
                  editing ? onOpenPage?.("memory") : show("memory")
                }
              >
                {!editing && open.has("memory") ? (
                  <textarea
                    autoFocus
                    value={memory}
                    rows={3}
                    maxLength={4000}
                    placeholder="One fact per line: the client, the deadline, where things live…"
                    onChange={(event) => setMemory(event.target.value)}
                    className={`mx-3 mb-3 resize-y py-2 leading-relaxed ${FIELD}`}
                  />
                ) : null}
              </ProjectRow>
              <ProjectRow
                icon={Clock}
                title="Scheduled tasks"
                note={
                  editing
                    ? "Habits the group runs on its own"
                    : addTask
                      ? "Its Habits open on a new task once it is created"
                      : "Habits the group runs on its own, on a schedule"
                }
                action={editing ? "Open" : addTask ? "Undo" : "Add"}
                onAction={() =>
                  editing ? onOpenPage?.("habits") : setAddTask(!addTask)
                }
              />
            </div>
            {problem ? (
              <p role="alert" className="text-[12px] text-red-400">
                {problem}
              </p>
            ) : null}
          </div>
        </div>
        <div className="flex shrink-0 justify-end gap-2 border-t border-stroke px-4 py-3">
          <button
            type="button"
            onClick={onClose}
            className="rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8 active:scale-[0.97]"
          >
            Cancel
          </button>
          <button
            type="submit"
            disabled={!ready}
            className="primary-action rounded-md px-3 py-1.5 text-[12px] font-medium active:scale-[0.97]"
          >
            {editing ? "Save" : "Create group"}
          </button>
        </div>
      </form>
    </Modal>
  );
}

/** One optional part of the project: a title, what it is, and its action. */
function ProjectRow({
  icon: Icon,
  title,
  note,
  action,
  onAction,
  children,
}: {
  icon: typeof Folder;
  title: string;
  note: string;
  action?: string;
  onAction: () => void;
  children?: ReactNode;
}) {
  return (
    <div className="flex min-w-0 flex-col">
      <div className="flex min-h-11 min-w-0 items-center gap-3 px-3 py-1.5">
        <Icon className="size-4 shrink-0 text-content/55" strokeWidth={1.75} />
        <span className="min-w-0 flex-1">
          <span className="block text-[13px] text-content/90">{title}</span>
          <span className="block truncate text-[11px] text-content/45">
            {note}
          </span>
        </span>
        {action ? (
          <button
            type="button"
            onClick={onAction}
            className="flex shrink-0 items-center gap-1 rounded-md px-2 py-1 text-[12px] text-content/65 hover:bg-content/8 hover:text-content"
          >
            {action === "Add" ? (
              <Plus className="size-3" strokeWidth={1.75} />
            ) : null}
            {action}
          </button>
        ) : null}
      </div>
      {children}
    </div>
  );
}
