// The desktop's session sync client, as the tests see it. It follows the
// native client in crates/monocode-core/src/remote/protocol.rs
// (`read_session_sync` and `Connections::load_session`): pieces of a chunked
// transfer are counted in UTF-16 units and rejected when empty, overlong or
// too many, and a delta that does not apply falls back to a snapshot.
import {
  applySessionSync,
  type HostSession,
  type SessionSync,
  type SessionSyncChunk,
  type SessionSyncResponse,
} from "../src/features/connections/model/protocol";

export type HostRequest = <T>(method: string, params: unknown) => Promise<T>;

const MAX_SESSION_SYNC_UNITS = 256 * 1024 * 1024;
const MAX_SESSION_SYNC_CHUNKS = 4096;

export async function readSessionSync(
  request: HostRequest,
  sessionId: string,
  revision?: number,
): Promise<SessionSync> {
  const response = await request<SessionSyncResponse>("sessions.sync", {
    sessionId,
    revision,
  });
  if (response.kind !== "chunked") return response;
  if (!response.transfer) throw new Error("Invalid session transfer");
  const { length } = response;
  if (!Number.isSafeInteger(length) || length <= 0 || length > MAX_SESSION_SYNC_UNITS)
    throw new Error("Session transfer has an invalid length");
  const pieces: string[] = [];
  let offset = 0;
  while (offset < length) {
    if (pieces.length >= MAX_SESSION_SYNC_CHUNKS)
      throw new Error("Session transfer has too many pieces");
    const { data } = await request<SessionSyncChunk>("sessions.syncChunk", {
      sessionId,
      transfer: response.transfer,
      offset,
    });
    if (typeof data !== "string" || !data)
      throw new Error("Session transfer ended early");
    offset += data.length;
    if (offset > length)
      throw new Error("Session transfer has an unexpected length");
    pieces.push(data);
  }
  return JSON.parse(pieces.join("")) as SessionSync;
}

/** Fetches only what changed since `known`; falls back to a full snapshot. */
export async function loadRemoteSession(
  request: HostRequest,
  sessionId: string,
  known?: HostSession,
): Promise<HostSession> {
  const update = await readSessionSync(request, sessionId, known?.revision);
  try {
    return applySessionSync(known, update);
  } catch {
    return applySessionSync(undefined, await readSessionSync(request, sessionId));
  }
}
