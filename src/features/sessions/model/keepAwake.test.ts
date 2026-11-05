import { describe, expect, it } from "vitest";
import { hasWorkUnderway } from "./keepAwake";
import { newSession, type Session } from "./session";

const session = (patch: Partial<Session> = {}): Session => ({
  ...newSession("claude", "/tmp/project"),
  ...patch,
});

describe("hasWorkUnderway", () => {
  it("is idle with nothing running or waiting", () => {
    expect(hasWorkUnderway([])).toBe(false);
    expect(hasWorkUnderway([session()])).toBe(false);
  });

  it("counts an agent at work", () => {
    expect(hasWorkUnderway([session(), session({ busy: true })])).toBe(true);
  });

  it("counts work waiting for its time", () => {
    expect(
      hasWorkUnderway([
        session({
          providerRetry: { reason: "server", attempt: 1, retryAt: 60_000 },
        }),
      ]),
    ).toBe(true);
    expect(
      hasWorkUnderway([
        session({ usageLimit: { resetsAt: 60_000, resumeAtReset: true } }),
      ]),
    ).toBe(true);
    expect(
      hasWorkUnderway([
        session({
          queuedMessages: [
            { id: "later", text: "Ship it", attachments: [], sendAt: 60_000 },
          ],
        }),
      ]),
    ).toBe(true);
  });

  it("ignores what won't go out by itself", () => {
    expect(
      hasWorkUnderway([
        session({ providerRetry: { reason: "server", attempt: 5 } }),
        session({ usageLimit: { resetsAt: 60_000 } }),
        session({
          queuedMessages: [{ id: "paused", text: "Next", attachments: [] }],
          queueStatus: "paused",
        }),
      ]),
    ).toBe(false);
  });

  it("leaves remote sessions to their host", () => {
    expect(
      hasWorkUnderway([session({ cwd: "remote://box/repo", busy: true })]),
    ).toBe(false);
  });
});
