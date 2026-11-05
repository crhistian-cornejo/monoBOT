import { useSyncExternalStore } from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { Lock } from "../../../shared/ui/icons";
import {
  monoForSession,
  monoLook,
  monosSnapshot,
  principalMono,
  saveGroup,
  subscribeMonos,
} from "../../monos/model/mono";
import {
  accessAnswerText,
  answerAccessRequest,
  isLink,
  type GroupAccess,
} from "../model/group";

/**
 * Above a group chat's composer: the Principal asking the user whether the
 * members may open a path or link outside the group's folders. Only the
 * user's tap grants it; the answer is saved on the group, so nobody asks
 * again, and goes to the chat so the group carries on.
 */
export function GroupAccessPrompt({
  sessionId,
  onReply,
}: {
  sessionId: string;
  onReply: (text: string) => void;
}) {
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const group = monoForSession(sessionId);
  const request = group?.group?.requests?.[0];
  if (!group || !request) return null;
  const waiting = group.group?.requests?.length ?? 0;
  const lead = principalMono();
  const asker = lead ? monoLook(lead).name : monoLook(group).name;
  const link = isLink(request.target);
  const answer = (access: GroupAccess) => {
    saveGroup(group.id, (current) =>
      answerAccessRequest(current, request.id, access),
    );
    onReply(accessAnswerText(request.target, access));
  };
  const button =
    "h-6 shrink-0 rounded-md px-2 text-[11px] text-content/65 hover:bg-content/10 hover:text-content";
  return (
    <div className="px-1.5 pb-1.5" data-group-access-prompt>
      <div className="rounded-lg border border-content/10 bg-content/3 px-3 py-2.5">
        <div className="flex items-center gap-1.5">
          <Lock
            className="size-3.5 shrink-0 text-content/45"
            strokeWidth={1.75}
          />
          <span className="min-w-0 flex-1 truncate text-[11px] text-content/50">
            {asker} asks for access
          </span>
          {waiting > 1 ? (
            <span className="shrink-0 text-[11px] text-content/40">
              1 of {waiting}
            </span>
          ) : null}
        </div>
        <p className="mt-1.5 text-[13px] leading-relaxed text-content/85">
          {request.by && request.by !== monoLook(group).name
            ? `${request.by} needs`
            : "The group needs"}{" "}
          {link ? "this link" : "this folder or file"}, outside the group’s
          folders:
        </p>
        <p
          className="mt-1 truncate font-mono text-[12px] text-content/70"
          title={request.target}
        >
          {link ? request.target : prettyCwd(request.target)}
        </p>
        {request.reason ? (
          <p className="mt-1 text-[12px] text-content/55">{request.reason}</p>
        ) : null}
        <div className="mt-2.5 flex flex-wrap items-center justify-end gap-2">
          <button type="button" className={button} onClick={() => answer("no")}>
            No
          </button>
          {link ? null : (
            <button
              type="button"
              className={button}
              onClick={() => answer("edit")}
            >
              Read and edit
            </button>
          )}
          <button
            type="button"
            onClick={() => answer("read")}
            className="h-6 rounded-md bg-content px-2.5 text-[11px] font-medium text-background-base hover:bg-content/80"
          >
            {link ? "Allow reading" : "Read only"}
          </button>
        </div>
      </div>
    </div>
  );
}
