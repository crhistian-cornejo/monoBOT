import type { HarnessId } from "./session";

/**
 * Per-project overrides for the desktop's Providers settings: which provider
 * new conversations start with, each provider's model, and which providers
 * show in the model picker. The desktop keeps them; a host session always
 * names its provider and model explicitly, so nothing is overridden here.
 */
export type ProjectProviderSettings = {
  defaultHarness?: HarnessId;
  defaultModel?: string;
  models?: Partial<Record<HarnessId, string>>;
  /** Providers this project keeps out of the picker and out of new sessions. */
  hidden?: HarnessId[];
};

export function loadProjectProviderSettings(
  _project: string | undefined,
): ProjectProviderSettings {
  return {};
}
