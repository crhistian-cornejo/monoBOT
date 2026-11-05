import { useCallback, useEffect, useId, useRef, useState } from "react";
import { prettyCwd } from "../../../shared/lib/paths";
import { Popover } from "../../../shared/ui/Popover";
import {
  BOT_ACCESS,
  BOT_SERVICES,
  botRole,
  botTools,
} from "../../bots/model/bots";
import { CONNECTORS } from "../../settings/model/connectors";
import { groupLead, groupMembers } from "../../groups/model/groupBrief";
import { GroupAvatarStack } from "../../groups/ui/GroupAvatarStack";
import { PreviewDetail } from "../../sessions/ui/SessionCardPreview";
import {
  MONO_STATUS_LABEL,
  findMono,
  isBot,
  monoLook,
  monoProjectsPhrase,
  type Mono,
  type MonoState,
} from "../model/mono";
import { MonoRailMascot } from "./MonoRailMascot";

const LIST_MAX = 4;

/** A few names and how many more, for one line of a card. */
function shortList(names: readonly string[]): string {
  const shown = names.slice(0, LIST_MAX).join(", ");
  return names.length > LIST_MAX
    ? `${shown} +${names.length - LIST_MAX}`
    : shown;
}

/** What a bot can use, grouped as its settings group it. */
function botToolLines(mono: Mono): { label: string; value: string }[] {
  const tools = botTools(mono.botRole ?? "assistant", mono.botTools);
  const accounts = BOT_SERVICES.filter((entry) =>
    tools.services.includes(entry.service),
  ).map((entry) => entry.name);
  const access = BOT_ACCESS.filter((entry) =>
    tools.access.includes(entry.access),
  ).map((entry) => entry.name);
  const connectors = CONNECTORS.filter((entry) =>
    tools.connectors?.includes(entry.id),
  ).map((entry) => entry.name);
  const folders = tools.folders.map((folder) => prettyCwd(folder.path));
  const lines = [
    { label: "Accounts", names: accounts },
    { label: "Can", names: access },
    { label: "Connectors", names: connectors },
    { label: "Skills", names: tools.skills ?? [] },
    { label: "Folders", names: folders },
  ];
  return lines.flatMap(({ label, names }) =>
    names.length ? [{ label, value: shortList(names) }] : [],
  );
}

/**
 * The hover card of a bot, a group or a Mono in the rail: what it is for,
 * who is in it and what it can use, in a few lines, as a session's card
 * tells its branch and folder in Code.
 */
export function MonoCardPreview({
  anchor,
  id,
  mono,
  state,
  onClose,
  onPointerEnter,
  onPointerLeave,
}: {
  anchor: HTMLElement;
  id: string;
  mono: Mono;
  state: MonoState;
  onClose: () => void;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
}) {
  useEffect(() => {
    const onScroll = (event: Event) => {
      if (
        event.target === document ||
        (event.target instanceof Element && event.target.contains(anchor))
      )
        onClose();
    };
    const onOtherCard = (event: PointerEvent) => {
      const card =
        event.target instanceof Element
          ? event.target.closest<HTMLElement>("[data-mono-card]")
          : null;
      if (card && card.dataset.monoCard !== mono.id) onClose();
    };
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("pointerover", onOtherCard, true);
    return () => {
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("pointerover", onOtherCard, true);
    };
  }, [anchor, onClose, mono.id]);

  const look = monoLook(mono);
  const group = mono.group;
  const kind = group
    ? `Group · ${group.members.length} ${
        group.members.length === 1 ? "member" : "members"
      }`
    : isBot(mono)
      ? mono.botRole === "principal"
        ? "Orchestrator"
        : `${botRole(mono.botRole ?? "assistant").name} bot`
      : mono.kind === "workstream"
        ? "Workstream"
        : "Mono";
  const about = group ? group.goal : look.specialty;
  const status = state.activity ?? MONO_STATUS_LABEL[state.status];
  const lead = group ? groupLead(group) : undefined;
  const members = group
    ? groupMembers(group)
        .filter((member) => member.id !== lead?.id)
        .map((member) => monoLook(member).name)
    : [];
  const waiting = group?.requests?.length ?? 0;

  return (
    <Popover
      anchor={anchor}
      side="right"
      align="start"
      gap={8}
      width={260}
      maxHeight={440}
      role="tooltip"
      id={id}
      data-mono-preview={mono.id}
      onDismiss={onClose}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
      className="overflow-y-auto overscroll-contain px-3 py-2.5"
    >
      <div className="flex items-center justify-between gap-3">
        <span className="flex min-w-0 items-center gap-1.5 text-[11px] text-content/55">
          {group ? (
            <GroupAvatarStack
              group={group}
              status={state.status}
              size="size-4"
            />
          ) : (
            <MonoRailMascot
              name={look.mascot}
              color={look.color}
              status={state.status}
              className="size-3.5 shrink-0"
            />
          )}
          <span className="min-w-0 truncate">{kind}</span>
        </span>
        <span
          className={`shrink-0 text-[11px] ${
            state.status === "needs-you"
              ? "text-accent"
              : state.status === "working"
                ? "text-content/70"
                : "text-content/40"
          }`}
        >
          {status}
        </span>
      </div>
      <p className="mt-1.5 break-words text-[13px] font-semibold leading-5 text-content">
        {look.name}
      </p>
      {about ? (
        <p className="mt-0.5 line-clamp-3 text-[12px] leading-[18px] text-content/65">
          {about}
        </p>
      ) : null}
      <dl className="mt-2 grid grid-cols-[4.5rem_minmax(0,1fr)] gap-x-2 gap-y-1.5 border-t border-stroke pt-2">
        {group ? (
          <>
            {lead ? (
              <PreviewDetail label="Leads">{monoLook(lead).name}</PreviewDetail>
            ) : null}
            <PreviewDetail label="Members">
              {members.length ? shortList(members) : "Only the lead yet"}
            </PreviewDetail>
            {group.folders?.length ? (
              <PreviewDetail label="Folders">
                {shortList(
                  group.folders.map((folder) => prettyCwd(folder.path)),
                )}
              </PreviewDetail>
            ) : null}
            {group.context?.length ? (
              <PreviewDetail label="Context">
                {group.context.length === 1
                  ? "1 file"
                  : `${group.context.length} files`}
              </PreviewDetail>
            ) : null}
            {waiting ? (
              <PreviewDetail label="Waiting">
                <span className="text-accent">
                  {waiting === 1
                    ? "1 access request"
                    : `${waiting} access requests`}
                </span>
              </PreviewDetail>
            ) : null}
          </>
        ) : isBot(mono) ? (
          botToolLines(mono).map((line) => (
            <PreviewDetail key={line.label} label={line.label}>
              {line.value}
            </PreviewDetail>
          ))
        ) : (
          <PreviewDetail label="Projects">
            {look.projects.length
              ? monoProjectsPhrase(look.projects)
              : "No projects yet"}
          </PreviewDetail>
        )}
      </dl>
    </Popover>
  );
}

const IDLE: MonoState = { status: "idle" };

/**
 * The hover card for rows of agents: `bind` goes on each row, `card` renders
 * the one under the pointer. The pointer may cross the gap into the card.
 */
export function useMonoCardPreview(
  states: ReadonlyMap<string, MonoState> | ((id: string) => MonoState),
) {
  const id = useId();
  const [preview, setPreview] = useState<{ id: string; anchor: HTMLElement }>();
  const closeTimer = useRef<number | null>(null);
  const cancelClose = useCallback(() => {
    if (closeTimer.current != null) {
      window.clearTimeout(closeTimer.current);
      closeTimer.current = null;
    }
  }, []);
  const close = useCallback(() => {
    cancelClose();
    setPreview(undefined);
  }, [cancelClose]);
  const scheduleClose = useCallback(() => {
    cancelClose();
    closeTimer.current = window.setTimeout(close, 120);
  }, [cancelClose, close]);
  useEffect(() => cancelClose, [cancelClose]);
  const bind = (monoId: string) => ({
    "data-mono-card": monoId,
    "aria-describedby": preview?.id === monoId ? id : undefined,
    onPointerEnter: (event: React.PointerEvent<HTMLElement>) => {
      if (event.pointerType === "touch") return;
      cancelClose();
      setPreview({ id: monoId, anchor: event.currentTarget });
    },
    onPointerLeave: () => {
      if (preview?.id === monoId) scheduleClose();
    },
  });
  const mono = preview ? findMono(preview.id) : undefined;
  const stateOf = (monoId: string) =>
    (typeof states === "function" ? states(monoId) : states.get(monoId)) ??
    IDLE;
  const card =
    preview && mono ? (
      <MonoCardPreview
        anchor={preview.anchor}
        id={id}
        mono={mono}
        state={stateOf(mono.id)}
        onClose={close}
        onPointerEnter={cancelClose}
        onPointerLeave={scheduleClose}
      />
    ) : null;
  return { bind, card, close };
}
