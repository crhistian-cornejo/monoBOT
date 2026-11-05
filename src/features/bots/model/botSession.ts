import { homeDir, listSkills } from "../../../platform/tauri/fs";
import { monoForSession } from "../../monos/model/mono";
import { botLaunch } from "./bots";

/** What a session's Claude process needs because it belongs to a bot, or
 * nothing for any other session. `installed` are the skills Claude Code
 * would load, so the ones the bot was not given can be denied. */
export function botLaunchForSession(
  sessionId: string,
  installed: readonly string[] = [],
) {
  const mono = monoForSession(sessionId);
  return mono?.kind === "bot"
    ? botLaunch(mono.botRole ?? "assistant", mono.botTools, installed)
    : null;
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
