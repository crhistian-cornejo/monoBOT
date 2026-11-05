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
  const field =
    "rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none placeholder:text-content/35 focus:border-content/25";
  const label = "flex flex-col gap-1.5 text-[12px] text-content/70";
  return (
    <Modal
      title="New group"
      description="Agents that work on one goal together"
      onClose={onClose}
      size="md"
    >
      <form
        className="flex flex-col gap-4 p-4"
        onSubmit={(event) => {
          event.preventDefault();
          if (!ready) return;
          onCreate({ name: name.trim(), goal: goal.trim(), members });
          onClose();
        }}
      >
        <label className={label}>
          Name
          <input
            autoFocus
            value={name}
            maxLength={60}
            placeholder="Morning briefing, Trip planning, Release crew…"
            autoComplete="off"
            onChange={(event) => setName(event.target.value)}
            className={`h-9 ${field}`}
          />
        </label>
        <label className={label}>
          What it is for
          <textarea
            value={goal}
            rows={2}
            maxLength={500}
            placeholder="Shared with every member when the group brings them in."
            onChange={(event) => setGoal(event.target.value)}
            className={`resize-none py-2 leading-relaxed ${field}`}
          />
        </label>
        <div className={label}>
          <span className="flex items-center">
            Members
            <span className="ml-auto tabular-nums text-content/35">
              {members.length}/{GROUP_MAX_MEMBERS}
            </span>
          </span>
          <MemberPicker selected={members} onChange={setMembers} />
        </div>
        <div className="flex justify-end gap-2">
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
            Create group
          </button>
        </div>
      </form>
    </Modal>
  );
}
