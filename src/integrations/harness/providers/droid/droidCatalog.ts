import { homeDir } from "../../../../platform/tauri/fs";
import {
  reportCatalogError,
  setHarnessModels,
} from "../../../../features/sessions/model/models";
import { AcpClient } from "../../core/acp";
import {
  killChild,
  resolveDroidBinary,
  spawnChild,
  unwatchChild,
  watchChild,
} from "../../core/child";
import { modelsFromSessionNew } from "../antigravity/antigravityProtocol";

const PROBE_ID = "monocode-droid-probe";
const REQUEST_TIMEOUT_MS = 20_000;
let inflight: Promise<void> | null = null;

/** Droid lists its models and their effort levels when a session opens. */
export function refreshDroidCatalog(): Promise<void> {
  if (inflight) return inflight;
  inflight = discoverDroidModels()
    .then((models) => {
      if (models.length > 0) setHarnessModels("droid", models);
    })
    .catch((error: unknown) => {
      // Keep the last live catalog (or the seeds) when offline or signed out.
      console.debug("[monocode] droid catalog", error);
      reportCatalogError("droid", error);
    })
    .finally(() => {
      inflight = null;
    });
  return inflight;
}

export async function discoverDroidModels(workingDirectory?: string) {
  const { path, args } = await resolveDroidBinary();
  const cwd = workingDirectory ?? (await homeDir());
  const probeId = `${PROBE_ID}-${crypto.randomUUID()}`;
  const acp = new AcpClient(probeId, {
    onRequest: (id, method) => {
      const response =
        method === "session/request_permission"
          ? acp.respond(id, { outcome: { outcome: "cancelled" } })
          : acp.respondError(id, {
              code: -32601,
              message: `Method not found: ${method}`,
            });
      void response.catch(() => undefined);
    },
  });
  watchChild(
    probeId,
    (line) => acp.pushLine(line),
    () => acp.close(new Error("Droid probe exited")),
  );
  try {
    await spawnChild(probeId, path, args, cwd, undefined, "droid");
    await acp.request(
      "initialize",
      {
        protocolVersion: 1,
        clientCapabilities: {
          fs: { readTextFile: false, writeTextFile: false },
          terminal: false,
          session: { configOptions: { boolean: {} } },
        },
        clientInfo: { name: "monocode", version: "0.1.0" },
      },
      REQUEST_TIMEOUT_MS,
    );
    // Signing in stays in Droid's own Terminal flow; never start it here.
    const created = await acp.request(
      "session/new",
      { cwd, mcpServers: [] },
      REQUEST_TIMEOUT_MS,
    );
    return modelsFromSessionNew(created, "droid");
  } finally {
    acp.close();
    unwatchChild(probeId);
    await killChild(probeId).catch(() => undefined);
  }
}
