import {
  BOT_FOLDERS_MAX,
  folderProblem,
  type BotFolder,
} from "../../bots/model/bots";

/**
 * A group brings agents together in one room: any of the user's Monos, bots
 * and workstreams. The group's own chat is its leader, the Principal (IVY)
 * running the room: it talks with the user and brings members in with the
 * app action group.ask, each member answering from its own conversation.
 *
 * Like a project, it may also have folders opened to its members, files
 * they read for context, and answers the user gave about paths and links
 * outside those folders. Its instructions, memory and scheduled tasks are
 * the group chat's own soul, memory and habits.
 */
export type Group = {
  /** Mono ids of the members, in the order they were added. */
  members: string[];
  /** What the group is for, shared with every member. */
  goal?: string;
  /** Folders opened to every member: read only, or read and change. */
  folders?: BotFolder[];
  /** Files every member reads for background when it matters. */
  context?: string[];
  /**
   * What the user answered when a member needed a path or link outside the
   * folders, so nobody asks twice. A granted folder also joins `folders`.
   */
  answers?: GroupAccessAnswer[];
  /** Asked and not answered yet: the group chat shows each one. */
  requests?: GroupAccessRequest[];
  /**
   * Each member's own conversation in this group, by member id: the same
   * agent, its tools and model, with a history apart from its own chat.
   */
  seats?: Record<string, string>;
};

/** read: may read; edit: may read and change; no: the user said no. */
export type GroupAccess = "read" | "edit" | "no";

export type GroupAccessAnswer = {
  /** A full path or an http(s) link. */
  target: string;
  access: GroupAccess;
  at: number;
};

export type GroupAccessRequest = {
  id: string;
  target: string;
  /** Who needs it, as the request names them. */
  by?: string;
  /** Why, in one line. */
  reason?: string;
  at: number;
};

export const GROUP_MAX_MEMBERS = 8;
export const GROUP_CONTEXT_MAX = 20;
const ANSWERS_MAX = 100;
const REQUESTS_MAX = 10;

const text = (value: unknown, max: number): string | undefined =>
  typeof value === "string" && value.trim()
    ? value.trim().slice(0, max)
    : undefined;

function parseFolders(value: unknown): BotFolder[] | undefined {
  if (!Array.isArray(value)) return undefined;
  const seen = new Set<string>();
  const folders = value.flatMap((entry: unknown) => {
    const folder = entry as Record<string, unknown> | null;
    if (!folder || typeof folder.path !== "string") return [];
    const path = folder.path.trim();
    const edit = folder.edit === true && !folderProblem(path, true);
    if (folderProblem(path, false) || seen.has(pathKey(path))) return [];
    seen.add(pathKey(path));
    return [{ path, edit }];
  });
  return folders.length ? folders.slice(0, BOT_FOLDERS_MAX) : undefined;
}

function isTarget(value: string): boolean {
  return isLink(value) || !folderProblem(value, false);
}

export function isLink(value: string): boolean {
  return /^https?:\/\/\S+$/i.test(value);
}

export function parseGroup(value: unknown): Group | undefined {
  if (!value || typeof value !== "object") return undefined;
  const entry = value as Record<string, unknown>;
  const members = Array.isArray(entry.members)
    ? [
        ...new Set(
          entry.members.filter(
            (id): id is string => typeof id === "string" && !!id,
          ),
        ),
      ].slice(0, GROUP_MAX_MEMBERS)
    : [];
  const goal = text(entry.goal, 500);
  const folders = parseFolders(entry.folders);
  const context = Array.isArray(entry.context)
    ? [
        ...new Set(
          entry.context.flatMap((path) => {
            const value = text(path, 1024);
            return value && !folderProblem(value, false) ? [value] : [];
          }),
        ),
      ].slice(0, GROUP_CONTEXT_MAX)
    : [];
  const answers = Array.isArray(entry.answers)
    ? entry.answers
        .flatMap((item: unknown) => {
          const answer = item as Record<string, unknown> | null;
          const target = text(answer?.target, 2048);
          const access = answer?.access;
          if (
            !target ||
            !isTarget(target) ||
            (access !== "read" && access !== "edit" && access !== "no")
          )
            return [];
          return [
            {
              target,
              access: access as GroupAccess,
              at: typeof answer?.at === "number" ? answer.at : 0,
            },
          ];
        })
        .slice(-ANSWERS_MAX)
    : [];
  const requests = Array.isArray(entry.requests)
    ? entry.requests
        .flatMap((item: unknown) => {
          const request = item as Record<string, unknown> | null;
          const id = text(request?.id, 64);
          const target = text(request?.target, 2048);
          if (!id || !target || !isTarget(target)) return [];
          const by = text(request?.by, 60);
          const reason = text(request?.reason, 300);
          return [
            {
              id,
              target,
              ...(by ? { by } : {}),
              ...(reason ? { reason } : {}),
              at: typeof request?.at === "number" ? request.at : 0,
            },
          ];
        })
        .slice(-REQUESTS_MAX)
    : [];
  const seats =
    entry.seats && typeof entry.seats === "object"
      ? Object.fromEntries(
          Object.entries(entry.seats as Record<string, unknown>).filter(
            (pair): pair is [string, string] =>
              members.includes(pair[0]) &&
              typeof pair[1] === "string" &&
              /^[A-Za-z0-9_-]{1,128}$/.test(pair[1]),
          ),
        )
      : {};
  return {
    members,
    ...(Object.keys(seats).length ? { seats } : {}),
    ...(goal ? { goal } : {}),
    ...(folders ? { folders } : {}),
    ...(context.length ? { context } : {}),
    ...(answers.length ? { answers } : {}),
    ...(requests.length ? { requests } : {}),
  };
}

/** A path as folders compare it: forward slashes, no trailing one, and
 * case-blind for a drive letter path, as Windows is. */
export function pathKey(path: string): string {
  const slashed = path.trim().replace(/\\/g, "/").replace(/\/+$/, "");
  return /^[a-z]:/i.test(slashed) ? slashed.toLowerCase() : slashed;
}

/** Whether `path` is `folder` or inside it. */
export function isInside(path: string, folder: string): boolean {
  const target = pathKey(path);
  const root = pathKey(folder);
  return target === root || target.startsWith(`${root}/`);
}

/**
 * What the members may do with a path or link: through the group's folders
 * or a saved answer, or undefined when nobody has asked the user yet.
 */
export function groupAccessFor(
  group: Group,
  target: string,
): GroupAccess | undefined {
  if (isLink(target)) {
    const link = target.trim();
    return [...(group.answers ?? [])]
      .reverse()
      .find((answer) => isLink(answer.target) && sameLink(answer.target, link))
      ?.access;
  }
  const folder = (group.folders ?? [])
    .filter((entry) => isInside(target, entry.path))
    .sort((a, b) => pathKey(b.path).length - pathKey(a.path).length)[0];
  if (folder) return folder.edit ? "edit" : "read";
  if ((group.context ?? []).some((path) => isInside(target, path)))
    return "read";
  return [...(group.answers ?? [])]
    .reverse()
    .find((answer) => !isLink(answer.target) && isInside(target, answer.target))
    ?.access;
}

/** Links compare by site and path, without a trailing slash or fragment. */
function sameLink(a: string, b: string): boolean {
  const key = (link: string) =>
    link.replace(/#.*$/, "").replace(/\/+$/, "").toLowerCase();
  return key(a) === key(b);
}

/** Adds a pending request, or returns the one already waiting for it. */
export function withAccessRequest(
  group: Group,
  request: GroupAccessRequest,
): { group: Group; request: GroupAccessRequest } {
  const waiting = (group.requests ?? []).find(
    (entry) => pathKey(entry.target) === pathKey(request.target),
  );
  if (waiting) return { group, request: waiting };
  return {
    group: {
      ...group,
      requests: [...(group.requests ?? []), request].slice(-REQUESTS_MAX),
    },
    request,
  };
}

/**
 * The user's answer to a request: saved so nobody asks again, and a folder
 * the user opened joins the group's folders with that access.
 */
export function answerAccessRequest(
  group: Group,
  requestId: string,
  access: GroupAccess,
  now = Date.now(),
): Group {
  const request = (group.requests ?? []).find(
    (entry) => entry.id === requestId,
  );
  if (!request) return group;
  const requests = (group.requests ?? []).filter(
    (entry) => entry.id !== requestId,
  );
  const answers = [
    ...(group.answers ?? []).filter(
      (answer) => pathKey(answer.target) !== pathKey(request.target),
    ),
    { target: request.target, access, at: now },
  ].slice(-ANSWERS_MAX);
  const opensFolder =
    access !== "no" &&
    !isLink(request.target) &&
    !folderProblem(request.target, access === "edit");
  const folders = opensFolder
    ? [
        ...(group.folders ?? []).filter(
          (folder) => pathKey(folder.path) !== pathKey(request.target),
        ),
        { path: request.target.trim(), edit: access === "edit" },
      ].slice(0, BOT_FOLDERS_MAX)
    : group.folders;
  const { requests: _, ...rest } = group;
  return {
    ...rest,
    ...(folders?.length ? { folders } : {}),
    answers,
    ...(requests.length ? { requests } : {}),
  };
}

/** What the user's answer says, as the group chat reads it. */
export function accessAnswerText(target: string, access: GroupAccess): string {
  const what = isLink(target) ? "this link" : "this path";
  return access === "no"
    ? `No: the group may not open ${what}: ${target}. Carry on without it.`
    : access === "edit"
      ? `Yes, read and edit: the members may read and change files in ${target} (never delete). Carry on.`
      : `Yes, read only: the members may read ${target} but not change it. Carry on.`;
}

/** The folders and files the members may use, as their briefs list them. */
export function groupPlaces(group: Group): string {
  const folders = (group.folders ?? []).map(
    (folder) =>
      `- ${folder.path} (${folder.edit ? "read and edit, never delete" : "read only"})`,
  );
  const context = (group.context ?? []).map((path) => `- ${path}`);
  const links = (group.answers ?? [])
    .filter((answer) => isLink(answer.target) && answer.access !== "no")
    .map((answer) => `- ${answer.target}`);
  return [
    folders.length
      ? `Folders open to the members:\n${folders.join("\n")}`
      : "No folders are open to the members.",
    ...(context.length
      ? [`Context files, read them when they bear on the work:\n${context.join("\n")}`]
      : []),
    ...(links.length ? [`Links the user allowed:\n${links.join("\n")}`] : []),
  ].join("\n");
}

/** The access rule every member and the leader keep. */
export const GROUP_ACCESS_RULE =
  'Stay inside the group\'s folders and files. Before you open a path or a link outside them, even one the user or another member sent, ask for it with app group.access {"target":"<full path or link>","reason":"<one line>"}: it answers at once when the user already decided; otherwise the user is asked in the group chat and you end your turn until they answer. Never work around a "no".';

const MEMORY_LINES_MAX = 60;
const MEMORY_CHARS_MAX = 6_000;

/** The project's live memory entries; the newest stay when it runs long. */
export function projectMemory(memory: string | undefined): string {
  const lines = (memory ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => /^[-*]\s/.test(line) && !line.includes("~~"))
    .slice(-MEMORY_LINES_MAX);
  while (lines.length > 1 && lines.join("\n").length > MEMORY_CHARS_MAX)
    lines.shift();
  return lines.join("\n");
}

/** What a member is told when the leader brings it in. */
export function groupMessage(
  groupName: string,
  goal: string | undefined,
  message: string,
  group?: Group,
  memory?: string,
): string {
  const remembered = projectMemory(memory);
  return `<group_message group="${groupName}">
${goal ? `The group is for: ${goal}\n` : ""}${group ? `${groupPlaces(group)}\n${GROUP_ACCESS_RULE}\n` : ""}${remembered ? `What the project remembers (keep to these decisions and preferences):\n${remembered}\n` : ""}${message.trim()}
</group_message>
Answer as yourself with your own contribution, briefly unless asked for more. You may ask another member of this group for something you need with app agents.message {"to":"<member>","message":"...","wait":true}, which waits for its reply. If you have nothing new to add, reply with exactly PASS.`;
}

/**
 * The folders an agent's groups open to it, each once with the widest
 * access any of its groups gives, and the context files they share.
 */
export function memberPlaces(groups: readonly Group[]): {
  folders: BotFolder[];
  context: string[];
} {
  const folders = new Map<string, BotFolder>();
  const context = new Map<string, string>();
  for (const group of groups) {
    for (const folder of group.folders ?? []) {
      const key = pathKey(folder.path);
      const known = folders.get(key);
      folders.set(key, {
        path: known?.path ?? folder.path,
        edit: !!known?.edit || folder.edit,
      });
    }
    for (const path of group.context ?? []) context.set(pathKey(path), path);
  }
  return { folders: [...folders.values()], context: [...context.values()] };
}
