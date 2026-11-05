import { buildOutgoingHandoffPrompt } from "./handoff";
import {
  cancelHarnessTurn,
  respondHarnessApproval,
  respondHarnessQuestion,
  sendHarnessTurn,
} from "../../../integrations/harness/core/registry";
import { mergeStream } from "../../../integrations/harness/core/streamText";
import type { HarnessId } from "./session";

/**
 * How long the outgoing agent gets to write its brief. Past it, the brief the
 * app builds from the transcript goes instead: the user is waiting on a
 * divider, not on the answer (a slow free model once held one 45 s).
 */
export const HANDOFF_TIMEOUT_MS = 20_000;
/** After cancelling, how long a turn gets to settle before it is let go. */
export const HANDOFF_SETTLE_MS = 3_000;

export async function requestOutgoingHandoff(input: {
  harness: HarnessId;
  sessionId: string;
  cwd: string;
  model: string;
  modelSettings?: Record<string, string>;
  providerAccountId?: string;
  userRequest: string;
}): Promise<string> {
  let brief = "";
  let giveUp: (() => void) | undefined;
  // A provider whose turn does not end on cancel must not hold the switch:
  // after the grace the wait ends with whatever brief streamed in.
  const abandoned = new Promise<void>((resolve) => {
    giveUp = resolve;
  });
  let grace: ReturnType<typeof setTimeout> | undefined;
  const timer = setTimeout(() => {
    void cancelHarnessTurn(input.harness, input.sessionId);
    grace = setTimeout(() => giveUp?.(), HANDOFF_SETTLE_MS);
  }, HANDOFF_TIMEOUT_MS);
  try {
    await Promise.race([abandoned, sendHarnessTurn({
      harness: input.harness,
      sessionId: input.sessionId,
      cwd: input.cwd,
      model: input.model,
      modelSettings: input.modelSettings,
      providerAccountId: input.providerAccountId,
      runtimeMode: "supervised",
      text: buildOutgoingHandoffPrompt(input.userRequest),
      onEvent: (event) => {
        if (event.type === "message.delta") {
          brief = mergeStream(brief, event.text);
        }
        if (event.type === "approval.requested") {
          respondHarnessApproval(
            input.harness,
            input.sessionId,
            event.requestId,
            "deny",
          );
        }
        if (event.type === "question.asked") {
          respondHarnessQuestion(
            input.harness,
            input.sessionId,
            event.requestId,
            { kind: "skipped" },
          );
        }
      },
    })]);
  } catch {
    // Caller falls back to the deterministic packet.
  } finally {
    clearTimeout(timer);
    if (grace) clearTimeout(grace);
  }
  return brief.trim();
}
