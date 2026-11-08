// Harness ids the arcade games draw. "droid" is left out on purpose: the
// native arcade has always shipped without it.
export type HarnessId =
  | "claude"
  | "codex"
  | "cursor"
  | "grok"
  | "opencode"
  | "pi"
  | "omp"
  | "fx"
  | "hermes"
  | "antigravity"
  | "devin"
  | "droid";

export const HARNESSES: HarnessId[] = ["claude", "codex", "cursor", "grok", "opencode", "pi", "omp", "fx", "hermes", "antigravity", "devin"];
