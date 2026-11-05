import {
  useEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  ImagePlus,
  MoreHorizontal,
  Plus,
  Settings,
  Trash2,
} from "../../shared/ui/icons";
import { Popover, type PopoverAnchor } from "../../shared/ui/Popover";
import { Shimmer } from "../../shared/ui/Shimmer";
import { useAnimatedReorder } from "../../shared/hooks/useAnimatedReorder";
import { MonoRailMascot } from "../../features/monos/ui/MonoRailMascot";
import { GroupAvatarStack } from "../../features/groups/ui/GroupAvatarStack";
import { useMonoCardPreview } from "../../features/monos/ui/MonoCardPreview";
import { MonoIntroCard } from "../../features/monos/ui/MonoIntro";
import { RailAction } from "./RailAction";
import { saveMonosEnabled } from "../../features/settings/model/settings";
import { ProjectBackgroundDialog } from "../../features/projects/ui/ProjectBackgroundDialog";
import { monoBackgroundKey } from "../../features/monos/model/monoBackground";
import {
  ColorPicker,
  MascotPicker,
} from "../../features/monos/ui/monoPanelParts";
import {
  defaultMonoName,
  findMono,
  isBot,
  listMonos,
  MONO_STATUS_LABEL,
  nextMonoLook,
  monoLook,
  monoProjectsPhrase,
  monosSnapshot,
  monoWorksOn,
  reorderMonos,
  saveMonoMascot,
  saveMonoName,
  subscribeMonos,
  updateMono,
  type MonoState,
} from "../../features/monos/model/mono";
import {
  BOT_ROLES,
  botIntroDismissed,
  dismissBotIntro,
  type BotRole,
} from "../../features/bots/model/bots";
import type { RailZone } from "./PersonalRail";
import { BetaBadge } from "../../shared/ui/BetaBadge";

export type MonoRailProps = {
  /** The Mono open in the main area, if any. */
  activeId?: string;
  /** What each Mono is doing, by Mono id; missing reads as idle. */
  states: ReadonlyMap<string, MonoState>;
  /** Replies each Mono has that the user has not looked at yet, by Mono id. */
  unseenCounts?: ReadonlyMap<string, number>;
  onOpen: (monoId: string) => void;
  /** Creates a Mono, given these projects to start with. */
  onCreate: (projects?: readonly string[]) => void;
  /** Creates a productivity bot with this role, or a blank one to set up. */
  onCreateBot?: (role: BotRole, options?: { blank?: boolean }) => void;
  onDelete: (monoId: string) => void;
  /** Opens the bots' news feed, on the Principal bot. */
  onOpenNews?: () => void;
  newsActive?: boolean;
  /** Which sidebar zone shows: Code (projects, Monos) or Personal (bots). */
  zone?: RailZone;
  onZoneChange?: (zone: RailZone) => void;
  /** Starts a new workstream (its tour first, the first time). */
  onCreateWorkstream?: () => void;
  /** Opens the dialog that makes a group. */
  onCreateGroup?: () => void;
  /** Opens a group's settings: name, goal, members, folders and the rest. */
  onEditGroup?: (monoId: string) => void;
};

const IDLE: MonoState = { status: "idle" };

/** A Mono's unread count as its badge shows it. */
export function unreadBadge(count: number): string {
  return count > 99 ? "99+" : String(count);
}

export function unreadLabel(count: number): string {
  return count === 1 ? "1 new reply" : `${count} new replies`;
}

/**
 * The user's Monos, above the projects. Each opens like a project does, but
 * into its conversation; none belongs to a project, so they get a section of
 * their own and the shaded mascot rather than a project's flat one.
 */
export function MonoRailSection({
  activeId,
  states,
  unseenCounts,
  onOpen,
  onCreate,
  onCreateBot,
  onCreateWorkstream,
  onCreateGroup,
  onEditGroup,
  onDelete,
  project,
  kind,
  inset = false,
}: MonoRailProps & {
  /**
   * Lists only the Monos working on this project, as the inset sidebar does;
   * a Mono on several projects shows in each of them.
   */
  project?: string;
  /** Lists the productivity bots or the workstreams instead of the Monos. */
  kind?: "bot" | "workstream" | "group";
  /** The focused layout's look without narrowing to a project. */
  inset?: boolean;
}) {
  // A project's section is always the focused sidebar's.
  const insetLook = inset || !!project;
  const bots = kind === "bot";
  const snapshot = useSyncExternalStore(subscribeMonos, monosSnapshot);
  const monos = useMemo(
    () =>
      listMonos().filter(
        (mono) =>
          (mono.kind ?? "mono") === (kind ?? "mono") &&
          // The Principal bot has its own row above the bots.
          mono.botRole !== "principal" &&
          (!project || monoWorksOn(mono, project)),
      ),
    [snapshot, project, bots],
  );
  const ids = monos.map((mono) => mono.id);
  const sortable = useAnimatedReorder(ids, reorderMonos, "y");
  const [menu, setMenu] = useState<{ id: string; anchor: PopoverAnchor }>();
  const [roles, setRoles] = useState<PopoverAnchor>();
  const section = useRef<HTMLDivElement>(null);
  const [background, setBackground] = useState<string>();
  // The hover card of the row under the pointer, as a session's in Code.
  const preview = useMonoCardPreview(states);
  // Invites a bot made from scratch until there is one or the card is closed.
  const [botIntroHidden, setBotIntroHidden] = useState(botIntroDismissed);
  const showBotIntro =
    bots &&
    !botIntroHidden &&
    !listMonos().some((mono) => isBot(mono) && mono.botRole === "custom");
  const backgroundMono = background ? findMono(background) : undefined;
  const workstreams = kind === "workstream";
  const groups = kind === "group";
  const label = bots
    ? "Bots"
    : workstreams
      ? "Workstreams"
      : groups
        ? "Groups"
        : "Monos";
  const newLabel = bots
    ? "New bot"
    : workstreams
      ? "New workstream"
      : groups
        ? "New group"
        : "New mono";
  const create = (anchor: HTMLElement) => {
    if (bots) setRoles(anchor);
    else if (workstreams) onCreateWorkstream?.();
    else if (groups) onCreateGroup?.();
    else onCreate(project ? [project] : undefined);
  };

  // With no Mono yet, the card stands in for the rail's section. A project
  // with none of its own offers one under the label instead.
  if (monos.length === 0 && !project && !kind)
    return (
      <div className="mb-2 shrink-0" data-mono-rail>
        <MonoIntroCard
          look={nextMonoLook(monos)}
          onCreate={() => onCreate()}
          onHide={() => saveMonosEnabled(false)}
        />
      </div>
    );

  return (
    <div
      ref={section}
      className={`shrink-0 ${insetLook ? "px-2 pb-3" : "mb-2"}`}
      data-mono-rail
      data-mono-project={project}
      data-mono-kind={kind}
    >
      <div
        className={
          // The inset sidebar's section labels.
          insetLook
            ? "flex h-7 items-center gap-1 px-2 text-[12px] font-medium text-content/50"
            : "flex items-center gap-1 px-3 pb-1.5 pt-1"
        }
      >
        <span
          className={`min-w-0 flex-1 truncate${
            insetLook ? "" : " px-1 text-xs leading-5 text-content/50"
          }`}
        >
          {label}
          {workstreams ? <BetaBadge className="ml-1.5 align-middle" /> : null}
        </span>
        {monos.length ? (
          <button
            type="button"
            title={newLabel}
            aria-label={newLabel}
            aria-haspopup={bots ? "menu" : undefined}
            onClick={(event) => create(event.currentTarget)}
            className={`grid size-5 shrink-0 place-items-center rounded-md text-content/50 hover:bg-content/8 hover:text-content${
              insetLook ? " -mr-0.5" : ""
            }`}
          >
            <Plus className="size-3.5" strokeWidth={1.75} />
          </button>
        ) : null}
      </div>
      <div className={`flex flex-col gap-px${insetLook ? "" : " px-2"}`}>
        {monos.length === 0 ? (
          <RailAction
            label={newLabel}
            icon={Plus}
            onClick={() => section.current && create(section.current)}
          />
        ) : null}
        {monos.map((mono) => {
          const look = monoLook(mono);
          const state = states.get(mono.id) ?? IDLE;
          const selected = mono.id === activeId;
          const unseen = selected ? 0 : (unseenCounts?.get(mono.id) ?? 0);
          // A group says what it is for; the others, where they work.
          const projects = look.group
            ? look.group.goal || `${look.group.members.length} members`
            : look.projects.length
              ? monoProjectsPhrase(look.projects)
              : "No projects yet";
          const status =
            state.status === "idle"
              ? undefined
              : (state.activity ?? MONO_STATUS_LABEL[state.status]);
          return (
            <div
              key={mono.id}
              ref={(el) => sortable.setItemRef(mono.id, el)}
              data-selected={selected || undefined}
              data-mono-status={state.status}
              {...preview.bind(mono.id)}
              className={`reorder-item project-reorder-item group relative flex h-8 cursor-default touch-none items-stretch rounded-md px-2 ${
                selected ? "bg-selection-strong text-content" : "opacity-65"
              }`}
              onPointerDown={(event) => {
                preview.close();
                if (event.button !== 0) return;
                if (
                  (event.target as HTMLElement | null)?.closest(
                    "[data-no-drag]",
                  )
                )
                  return;
                sortable.onItemPointerDown(mono.id, event);
              }}
              onClick={(event) => {
                if (
                  (event.target as HTMLElement | null)?.closest(
                    "[data-no-drag]",
                  )
                )
                  return;
                if (sortable.consumeClick()) return;
                onOpen(mono.id);
              }}
              onContextMenu={(event) => {
                event.preventDefault();
                preview.close();
                setMenu({
                  id: mono.id,
                  anchor: { x: event.clientX, y: event.clientY },
                });
              }}
            >
              <button
                type="button"
                aria-label={[
                  look.name,
                  status ?? "idle",
                  unseen ? unreadLabel(unseen) : undefined,
                  projects,
                ]
                  .filter(Boolean)
                  .join(", ")}
                aria-current={selected ? "true" : undefined}
                className="flex min-w-0 flex-1 cursor-default items-center gap-2 text-left transition-[padding] duration-150 motion-reduce:transition-none group-hover:pr-6 group-has-[button:focus-visible]:pr-6"
              >
                {look.group ? (
                  <GroupAvatarStack group={look.group} status={state.status} />
                ) : (
                  <MonoRailMascot
                    name={look.mascot}
                    color={look.color}
                    status={state.status}
                  />
                )}
                {state.status === "working" ? (
                  <Shimmer
                    as="span"
                    duration={1.4}
                    className="min-w-0 flex-1 truncate text-sm font-medium leading-tight"
                  >
                    {look.name}
                  </Shimmer>
                ) : (
                  <span className="min-w-0 flex-1 truncate text-sm font-medium leading-tight">
                    {look.name}
                  </span>
                )}
                {unseen ? (
                  <span
                    aria-hidden
                    data-mono-unread={unseen}
                    className="grid h-4 min-w-4 shrink-0 place-items-center rounded-full bg-accent px-1 text-[10px] font-medium tabular-nums leading-none text-white group-hover:hidden group-has-[button:focus-visible]:hidden"
                  >
                    {unreadBadge(unseen)}
                  </span>
                ) : null}
              </button>
              <button
                type="button"
                data-no-drag
                title="Mono options"
                aria-label={`${look.name} options`}
                aria-haspopup="menu"
                onPointerDown={(event) => event.stopPropagation()}
                onClick={(event) => {
                  event.stopPropagation();
                  setMenu({ id: mono.id, anchor: event.currentTarget });
                }}
                className="absolute right-1 top-1/2 hidden size-6 -translate-y-1/2 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content group-hover:grid group-has-[button:focus-visible]:grid"
              >
                <MoreHorizontal className="size-4" strokeWidth={1.75} />
              </button>
            </div>
          );
        })}
      </div>
      {menu ? null : preview.card}
      {showBotIntro ? (
        // The card brings its own side margin; the inset section has one too.
        <div className={insetLook ? "-mx-2 mt-2" : "mt-2"}>
          <MonoIntroCard
            look={nextMonoLook(listMonos())}
            title="A bot of your own"
            body="Beyond Assistant, Finance, Documents and Research: give it a name, write its soul and pick its tools."
            createLabel="Create bot"
            hideAs="close"
            onCreate={() => onCreateBot?.("custom", { blank: true })}
            onHide={() => {
              dismissBotIntro();
              setBotIntroHidden(true);
            }}
          />
        </div>
      ) : null}
      {menu ? (
        <MonoMenu
          key={menu.id}
          monoId={menu.id}
          anchor={menu.anchor}
          onBackground={() => {
            setMenu(undefined);
            setBackground(menu.id);
          }}
          onDelete={() => {
            setMenu(undefined);
            onDelete(menu.id);
          }}
          onSettings={
            groups && onEditGroup
              ? () => {
                  setMenu(undefined);
                  onEditGroup(menu.id);
                }
              : undefined
          }
          onClose={() => setMenu(undefined)}
        />
      ) : null}
      {roles ? (
        <Popover
          anchor={roles}
          side="right"
          align="start"
          width={260}
          onDismiss={() => setRoles(undefined)}
          role="menu"
          aria-label="New bot"
          className="p-1"
        >
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setRoles(undefined);
              onCreateBot?.("custom", { blank: true });
            }}
            className="flex w-full flex-col items-start px-2.5 py-1.5 text-left hover:bg-content/8"
          >
            <span className="text-[13px] font-medium">Blank bot</span>
            <span className="text-xs text-content/55">
              Start from scratch: name, soul, model
            </span>
          </button>
          <div role="separator" className="my-1 h-px bg-content/10" />
          {BOT_ROLES.filter((info) => !info.hidden).map((info) => (
            <button
              key={info.role}
              type="button"
              role="menuitem"
              onClick={() => {
                setRoles(undefined);
                onCreateBot?.(info.role);
              }}
              className="flex w-full flex-col items-start px-2.5 py-1.5 text-left hover:bg-content/8"
            >
              <span className="text-[13px] font-medium">{info.name}</span>
              <span className="text-xs text-content/55">
                {info.description}
              </span>
            </button>
          ))}
        </Popover>
      ) : null}
      {backgroundMono ? (
        <ProjectBackgroundDialog
          project={monoBackgroundKey(backgroundMono.id)}
          name={monoLook(backgroundMono).name}
          locked
          onClose={() => setBackground(undefined)}
        />
      ) : null}
    </div>
  );
}

/**
 * Right-click menu for a Mono, like a project's: its name, mascot and color
 * up top, edited in place, then the actions.
 */
function MonoMenu({
  monoId,
  anchor,
  onBackground,
  onDelete,
  onSettings,
  onClose,
}: {
  monoId: string;
  anchor: PopoverAnchor;
  onBackground: () => void;
  onDelete: () => void;
  /** A group's settings; other Monos have theirs in their chat. */
  onSettings?: () => void;
  onClose: () => void;
}) {
  useSyncExternalStore(subscribeMonos, monosSnapshot);
  const mono = findMono(monoId);
  const input = useRef<HTMLInputElement>(null);
  const [name, setName] = useState(mono?.name ?? "");

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, []);

  if (!mono) return null;
  const look = monoLook(mono);
  const commitName = () => {
    const next = name.trim().slice(0, 40);
    if (next !== (findMono(monoId)?.name ?? "")) saveMonoName(monoId, next);
  };

  return (
    <Popover
      anchor={anchor}
      side="right"
      align="start"
      gap={0}
      width={280}
      constrainHeight={false}
      onDismiss={() => {
        commitName();
        onClose();
      }}
      role="menu"
      aria-label="Mono options"
      onContextMenu={(event) => event.preventDefault()}
      className="p-2"
    >
      <input
        ref={input}
        value={name}
        placeholder={defaultMonoName(mono.mascot)}
        maxLength={40}
        aria-label="Mono name"
        onChange={(event) => setName(event.target.value)}
        onBlur={commitName}
        onKeyDown={(event) => {
          if (event.key !== "Enter") return;
          event.preventDefault();
          commitName();
          onClose();
        }}
        className="mb-2 w-full rounded-lg border border-content/10 bg-content/5 px-2.5 py-1.5 text-[13px] text-content outline-none ring-accent/40 placeholder:text-content/45 focus:ring-1"
      />
      <div className="mb-2 flex flex-col gap-2">
        <MascotPicker
          current={look.mascot}
          color={look.color}
          onPick={(mascot) => saveMonoMascot(monoId, mascot)}
        />
        <ColorPicker
          inline
          current={look.color}
          onPick={(color) => updateMono(monoId, (mono) => ({ ...mono, color }))}
        />
      </div>
      <div role="separator" className="my-1 h-px bg-content/10" />
      {onSettings ? (
        <button
          type="button"
          role="menuitem"
          onClick={() => {
            commitName();
            onSettings();
          }}
          className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] text-content/85 hover:bg-content/8"
        >
          <Settings className="size-3.5 shrink-0" strokeWidth={1.75} />
          Group settings…
        </button>
      ) : null}
      <button
        type="button"
        role="menuitem"
        onClick={() => {
          commitName();
          onBackground();
        }}
        className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] text-content/85 hover:bg-content/8"
      >
        <ImagePlus className="size-3.5 shrink-0" strokeWidth={1.75} />
        Background image
      </button>
      <button
        type="button"
        role="menuitem"
        onClick={onDelete}
        className="flex w-full items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-left text-[13px] text-red-400 hover:bg-content/8"
      >
        <Trash2 className="size-3.5 shrink-0" strokeWidth={1.75} />
        Delete mono…
      </button>
    </Popover>
  );
}
