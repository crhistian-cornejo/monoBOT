import type { BotModel } from "../../bots/model/botTransfer";
import type { Session } from "../../sessions/model/session";

type ModelSide = Pick<
  Session,
  "harness" | "model" | "modelSettings" | "runtimeMode"
>;

/**
 * What a group's chat should switch to so it answers with its Principal's
 * model, settings and permissions, or undefined when it already does.
 */
export function leadModelChoice(
  group: ModelSide,
  lead: ModelSide,
): BotModel | undefined {
  const same =
    group.harness === lead.harness &&
    group.model === lead.model &&
    group.runtimeMode === lead.runtimeMode &&
    JSON.stringify(group.modelSettings ?? {}) ===
      JSON.stringify(lead.modelSettings ?? {});
  if (same || !lead.model) return undefined;
  return {
    harness: lead.harness,
    model: lead.model,
    modelSettings: { ...(lead.modelSettings ?? {}) },
    runtimeMode: lead.runtimeMode,
  };
}
