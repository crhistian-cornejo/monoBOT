import { afterEach, beforeEach, expect, it, vi } from "vitest";

const cancelHarnessTurn = vi.fn(async () => undefined);
let sendHarnessTurn: (input: {
  onEvent: (event: { type: string; text?: string }) => void;
}) => Promise<void>;

vi.mock("../../../integrations/harness/core/registry", () => ({
  cancelHarnessTurn: (...args: unknown[]) => cancelHarnessTurn(...(args as [])),
  respondHarnessApproval: vi.fn(),
  respondHarnessQuestion: vi.fn(),
  sendHarnessTurn: (input: never) => sendHarnessTurn(input),
}));

const { HANDOFF_SETTLE_MS, HANDOFF_TIMEOUT_MS, requestOutgoingHandoff } =
  await import("./handoffTurn");

const input = {
  harness: "opencode" as const,
  sessionId: "ivy",
  cwd: "/Users/me",
  model: "opencode:opencode/big-pickle",
  userRequest: "hey",
};

beforeEach(() => {
  vi.useFakeTimers();
  cancelHarnessTurn.mockClear();
});
afterEach(() => vi.useRealTimers());

it("returns the brief the outgoing agent wrote", async () => {
  sendHarnessTurn = async ({ onEvent }) => {
    onEvent({ type: "message.delta", text: "We were rendering the panel." });
  };
  await expect(requestOutgoingHandoff(input)).resolves.toBe(
    "We were rendering the panel.",
  );
  expect(cancelHarnessTurn).not.toHaveBeenCalled();
});

it("stops waiting on an agent whose turn never ends, keeping what it wrote", async () => {
  // A slow model that streams a little, then never finishes, even when cancelled.
  sendHarnessTurn = ({ onEvent }) => {
    onEvent({ type: "message.delta", text: "Partial brief" });
    return new Promise(() => undefined);
  };
  let settled: string | undefined;
  void requestOutgoingHandoff(input).then((brief) => {
    settled = brief;
  });
  await vi.advanceTimersByTimeAsync(HANDOFF_TIMEOUT_MS);
  expect(cancelHarnessTurn).toHaveBeenCalledWith("opencode", "ivy");
  expect(settled).toBeUndefined();
  await vi.advanceTimersByTimeAsync(HANDOFF_SETTLE_MS);
  expect(settled).toBe("Partial brief");
  // The whole switch waits well under the 45 s it once took.
  expect(HANDOFF_TIMEOUT_MS + HANDOFF_SETTLE_MS).toBeLessThan(30_000);
});
