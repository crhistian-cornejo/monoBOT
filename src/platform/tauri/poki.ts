import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  PokiInstallStage,
  PokiMachine,
} from "../../features/settings/model/pokiSetup";

/** What this Mac has for Poki, and whether MonoBOT installed or runs it. */
export function pokiMachine(): Promise<PokiMachine> {
  return invoke<PokiMachine>("poki_setup");
}

/** Downloads the pinned release, checks its digest and unpacks it; returns the folder. */
export function installPoki(): Promise<string> {
  return invoke<string>("poki_install");
}

/** Starts Poki's daemon from a folder, unless one already runs. */
export function startPoki(dir: string): Promise<void> {
  return invoke<void>("poki_start", { dir });
}

/** Stops the daemon MonoBOT started; one the user started stays up. */
export function stopPoki(): Promise<void> {
  return invoke<void>("poki_stop");
}

export function onPokiInstall(
  handler: (stage: PokiInstallStage) => void,
): Promise<UnlistenFn> {
  return listen<{ stage: PokiInstallStage }>("poki-install", (event) =>
    handler(event.payload.stage),
  );
}
