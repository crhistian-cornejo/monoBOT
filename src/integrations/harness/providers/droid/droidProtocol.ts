import type { RuntimeMode } from "../../../../features/sessions/model/session";

/**
 * Factory's Droid speaks standard ACP through `droid exec -o acp`: models,
 * reasoning effort and autonomy are session config options, permissions
 * use the ACP option kinds, and sessions resume by id. What is Droid's own
 * lives here; the wire format is shared with the other ACP providers.
 */

/** The arguments that start Droid's ACP server on standard input and output. */
export const DROID_ACP_ARGS = ["exec", "-o", "acp"] as const;

/** Droid's autonomy levels, as `session/set_mode` takes them. */
export type DroidModeId =
  | "normal"
  | "spec"
  | "auto-low"
  | "auto-medium"
  | "auto-high";

/**
 * MonoBOT's permission mode as a Droid autonomy level. Supervised lets Droid
 * read on its own and asks for everything else; full access lets it run all.
 * Planning uses Droid's own Spec mode, which is read-only.
 */
export function droidModeId(
  runtimeMode: RuntimeMode,
  planning = false,
): DroidModeId {
  if (planning) return "spec";
  if (runtimeMode === "full-access") return "auto-high";
  if (runtimeMode === "auto") return "auto-medium";
  if (runtimeMode === "auto-accept-edits") return "auto-low";
  return "normal";
}

export const DROID_AUTH_HELP =
  "Run `droid` once in Terminal to sign in to Factory, then retry.";

/** An error as the user reads it, with the sign-in step when it is about that. */
export function droidError(error: unknown): Error {
  const detail = error instanceof Error ? error.message : String(error);
  if (detail.includes(DROID_AUTH_HELP)) return new Error(detail);
  return new Error(
    /auth|login|sign.in|credential|api.key|unauthori[sz]ed|pairing/i.test(detail)
      ? `${detail.trim()}\n\n${DROID_AUTH_HELP}`
      : detail,
  );
}
