import { monoForSession } from "../../monos/model/mono";
import { botLaunch } from "./bots";

/** What a session's Claude process needs because it belongs to a bot, or
 * nothing for any other session. */
export function botLaunchForSession(sessionId: string) {
  const mono = monoForSession(sessionId);
  return mono?.kind === "bot" ? botLaunch(mono.botRole ?? "assistant") : null;
}
