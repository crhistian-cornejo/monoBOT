import { useState } from "react";
import { Modal } from "../../../shared/ui/Modal";
import { GROUP_MAX_MEMBERS } from "../model/group";
import { MemberPicker } from "./GroupMembers";

/** Names a group, says what it is for, and picks who is in it. */
export function NewGroupDialog({
  onCreate,
  onClose,
}: {
  onCreate: (input: { name: string; goal: string; members: string[] }) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState("");
  const [goal, setGoal] = useState("");
  const [members, setMembers] = useState<string[]>([]);
  const ready = !!name.trim() && members.length > 0;
  const label = "font-mono text-[11px] uppercase tracking-wide text-content/55";
  return (
    <Modal title="New group" onClose={onClose} size="md">
      <form
        className="flex flex-col gap-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (!ready) return;
          onCreate({ name: name.trim(), goal: goal.trim(), members });
          onClose();
        }}
      >
        <label className="flex flex-col gap-1.5">
          <span className={label}>Group name *</span>
          <input
            autoFocus
            value={name}
            maxLength={60}
            placeholder="Morning briefing, Trip planning, Release crew…"
            onChange={(event) => setName(event.target.value)}
            className="h-9 border border-stroke bg-transparent px-2.5 text-[13px] outline-none focus:border-accent"
          />
        </label>
        <label className="flex flex-col gap-1.5">
          <span className={label}>What it is for</span>
          <textarea
            value={goal}
            rows={2}
            maxLength={500}
            placeholder="Shared with every member when the group brings them in."
            onChange={(event) => setGoal(event.target.value)}
            className="resize-none border border-stroke bg-transparent px-2.5 py-2 text-[13px] outline-none focus:border-accent"
          />
        </label>
        <div className="flex flex-col gap-1.5">
          <span className={label}>
            Members * · {members.length}/{GROUP_MAX_MEMBERS}
          </span>
          <MemberPicker selected={members} onChange={setMembers} />
        </div>
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
            Create group
          </button>
        </div>
      </form>
    </Modal>
  );
}
