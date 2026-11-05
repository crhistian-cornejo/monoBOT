import { invoke } from "@tauri-apps/api/core";
import { isRemoteProjectPath } from "../../projects/model/recents";
import type { Session } from "./session";

/**
 * Work that stalls if the computer sleeps: an agent working, a provider retry
 * or a usage-limit resume waiting, or a message scheduled with Send later.
 * A remote session's work runs on its host, so it doesn't count.
 */
export function hasWorkUnderway(sessions: readonly Session[]): boolean {
  return sessions.some(
    (session) =>
      !isRemoteProjectPath(session.cwd) &&
      (!!session.busy ||
        session.providerRetry?.retryAt != null ||
        !!session.usageLimit?.resumeAtReset ||
        !!session.queuedMessages?.some((message) => message.sendAt != null)),
  );
}

/** Hold off idle sleep, or let it happen again. Repeating either is harmless. */
export function setKeepAwake(enabled: boolean): Promise<void> {
  return invoke("keep_awake_set", { enabled });
}
