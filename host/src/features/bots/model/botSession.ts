import { homeDir, listSkills } from "../../../platform/tauri/fs";
import { groupsOf, monoForSession, type Mono } from "../../monos/model/mono";
import { memberPlaces, pathKey } from "../../groups/model/group";
import { botLaunch, botTools, rulePath, type BotFolder } from "./bots";

/** The folders and files the agent's groups open to it; a group's own
 * chat gets its group's. */
function groupPlacesFor(mono: Mono) {
  const groups =
    mono.kind === "group" && mono.group
      ? [mono.group]
      : groupsOf(mono.id).flatMap((group) => group.group ?? []);
  return memberPlaces(groups);
}

/** Read-only group folders made a rule Claude Code enforces. */
function readOnlyRules(folders: readonly BotFolder[]): string[] {
  return folders.flatMap((folder) => {
    if (folder.edit) return [];
    const at = `//${rulePath(folder.path)}/**`;
    return [`Edit(${at})`, `Write(${at})`];
  });
}

const contextRules = (context: readonly string[]) =>
  context.map((path) => `Read(//${rulePath(path)})`);

/** What a session's Claude process needs because it belongs to a bot, or
 * nothing for any other session. `installed` are the skills Claude Code
 * would load, so the ones the bot was not given can be denied. A bot in a
 * group also gets the group's folders, at the group's access where its own
 * tools let it change files at all. */
export function botLaunchForSession(
  sessionId: string,
  installed: readonly string[] = [],
) {
  const mono = monoForSession(sessionId);
  if (mono?.kind !== "bot") return null;
  const role = mono.botRole ?? "assistant";
  const places = groupPlacesFor(mono);
  if (!places.folders.length && !places.context.length)
    return botLaunch(role, mono.botTools, installed);
  const tools = botTools(role, mono.botTools);
  const own = new Set(tools.folders.map((folder) => pathKey(folder.path)));
  const added = places.folders.filter(
    (folder) => !own.has(pathKey(folder.path)),
  );
  const launch = botLaunch(
    role,
    { ...tools, folders: [...tools.folders, ...added] },
    installed,
  );
  return {
    ...launch,
    allowedTools: [...launch.allowedTools, ...contextRules(places.context)],
    disallowedTools: [...launch.disallowedTools, ...readOnlyRules(added)],
  };
}

/**
 * What any other session in a group needs: the group chat and the coding
 * Monos in groups get the groups' folders, read only where the group says.
 * The group chat leads rather than launching subagents of its own: its
 * members are its helpers, and their subagents show under them.
 */
export function groupLaunchForSession(sessionId: string): {
  allowedTools: string[];
  disallowedTools: string[];
  addDirs: string[];
} | null {
  const mono = monoForSession(sessionId);
  if (!mono || mono.kind === "bot") return null;
  const lead = mono.kind === "group";
  const { folders, context } = groupPlacesFor(mono);
  if (!lead && !folders.length && !context.length) return null;
  return {
    allowedTools: [
      ...folders.flatMap((folder) => {
        const at = `//${rulePath(folder.path)}/**`;
        return [
          `Read(${at})`,
          ...(folder.edit ? [`Edit(${at})`, `Write(${at})`] : []),
        ];
      }),
      ...contextRules(context),
    ],
    disallowedTools: [
      ...readOnlyRules(folders),
      ...(lead ? ["Task", "Agent"] : []),
    ],
    addDirs: folders.map((folder) => folder.path),
  };
}

/** Whether the session is a bot's, whose skills are narrowed. */
export function isBotSession(sessionId: string): boolean {
  return monoForSession(sessionId)?.kind === "bot";
}

/**
 * The skills Claude Code loads for a session in `cwd`: the user's and the
 * project's own, and those of the plugins they installed. Empty when they
 * cannot be listed, which leaves every skill open, as before.
 */
export async function installedClaudeSkills(cwd: string): Promise<string[]> {
  try {
    const skills = await listSkills(cwd);
    return skills
      .filter((skill) => skill.source === "claude" || skill.source === "agents")
      .map((skill) => skill.name);
  } catch {
    return [];
  }
}

/** The skills installed for the user, as a bot's chat (in home) loads them. */
export async function userInstalledSkills(): Promise<string[]> {
  try {
    return installedClaudeSkills(await homeDir());
  } catch {
    return [];
  }
}
