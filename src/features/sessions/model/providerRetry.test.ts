import { describe, expect, it } from "vitest";
import { newSession, type Session } from "./session";
import {
  PROVIDER_RETRY_MAX,
  nextProviderRetry,
  providerRetryDue,
  providerRetryReason,
  providerRetryStatus,
  providerRetrySucceeded,
  providerRetryWait,
  sendingProviderRetry,
} from "./providerRetry";

function session(patch: Partial<Session> = {}): Session {
  return { ...newSession("claude", "/tmp/project"), ...patch };
}

describe("providerRetryReason", () => {
  it("retries overloads, server errors and dropped connections", () => {
    expect(
      providerRetryReason(
        'API Error: 529 {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}',
      ),
    ).toBe("overloaded");
    expect(providerRetryReason("We're currently experiencing high demand")).toBe(
      "overloaded",
    );
    expect(providerRetryReason("API Error: 500 Internal server error")).toBe(
      "server",
    );
    expect(providerRetryReason("unexpected status 503 Service Unavailable")).toBe(
      "server",
    );
    expect(providerRetryReason("HTTP 502 Bad Gateway")).toBe("server");
    expect(providerRetryReason("read ECONNRESET")).toBe("connection");
    expect(
      providerRetryReason("stream disconnected before completion"),
    ).toBe("connection");
    expect(providerRetryReason("Request timed out.")).toBe("connection");
  });

  it("never retries limits, auth or the user's own errors", () => {
    expect(providerRetryReason("Claude AI usage limit reached|1760000000")).toBe(
      undefined,
    );
    expect(providerRetryReason("Your credit balance is too low")).toBe(
      undefined,
    );
    expect(providerRetryReason("insufficient_quota")).toBe(undefined);
    expect(providerRetryReason("API Error: 401 invalid x-api-key")).toBe(
      undefined,
    );
    expect(providerRetryReason("Prompt is too long")).toBe(undefined);
    expect(providerRetryReason("Listening on port 5000")).toBe(undefined);
  });
});

describe("providerRetryWait", () => {
  it("doubles each time, starting at a minute for server errors", () => {
    expect(
      [1, 2, 3, 4].map((retry) => providerRetryWait("server", retry, 0)),
    ).toEqual([60_000, 120_000, 240_000, 480_000]);
    expect(providerRetryWait("connection", 1, 0)).toBe(15_000);
  });

  it("adds up to a quarter at random", () => {
    expect(providerRetryWait("server", 1, 0.999)).toBeLessThan(75_000);
    expect(providerRetryWait("server", 1, 0.5)).toBe(67_500);
  });
});

describe("nextProviderRetry", () => {
  it("schedules the first retry", () => {
    expect(nextProviderRetry(session(), "API Error: 529 Overloaded", 1_000, 0)).toEqual({
      reason: "overloaded",
      attempt: 1,
      retryAt: 61_000,
    });
  });

  it("counts a failed retry as the next attempt", () => {
    const retrying = session({
      providerRetry: { reason: "server", attempt: 2, sending: true },
    });
    expect(nextProviderRetry(retrying, "HTTP 502 Bad Gateway", 0, 0)).toEqual({
      reason: "server",
      attempt: 3,
      retryAt: 240_000,
    });
  });

  it("doesn't count a second error from the same failed turn", () => {
    const failed = session({
      providerRetry: { reason: "server", attempt: 1, retryAt: 60_000 },
    });
    expect(nextProviderRetry(failed, "read ECONNRESET", 5_000, 0)).toBe(
      failed.providerRetry,
    );
  });

  it("stops retrying by itself after the last attempt", () => {
    const last = session({
      providerRetry: {
        reason: "server",
        attempt: PROVIDER_RETRY_MAX,
        sending: true,
      },
    });
    expect(nextProviderRetry(last, "HTTP 500", 0, 0)).toEqual({
      reason: "server",
      attempt: PROVIDER_RETRY_MAX + 1,
    });
  });

  it("leaves scheduled runs and usage limits alone", () => {
    expect(
      nextProviderRetry(session({ automationId: "daily" }), "HTTP 500", 0, 0),
    ).toEqual({ reason: "server", attempt: 1 });
    expect(
      nextProviderRetry(session({ ephemeral: true }), "HTTP 500", 0, 0)?.retryAt,
    ).toBeUndefined();
    expect(
      nextProviderRetry(session({ usageLimit: {} }), "HTTP 500", 0, 0),
    ).toBeUndefined();
  });

  it("clears the retry when the turn fails for another reason", () => {
    const retrying = session({
      providerRetry: { reason: "server", attempt: 1, sending: true },
    });
    expect(nextProviderRetry(retrying, "Prompt is too long", 0, 0)).toBe(
      undefined,
    );
  });
});

describe("retry lifecycle", () => {
  it("is due once idle and past the wait", () => {
    const waiting = session({
      providerRetry: { reason: "server", attempt: 1, retryAt: 60_000 },
    });
    expect(providerRetryDue(waiting, 59_999)).toBe(false);
    expect(providerRetryDue(waiting, 60_000)).toBe(true);
    expect(providerRetryDue({ ...waiting, busy: true }, 60_000)).toBe(false);
    expect(
      providerRetryDue(
        session({ providerRetry: { reason: "server", attempt: 5 } }),
        Number.MAX_SAFE_INTEGER,
      ),
    ).toBe(false);
  });

  it("succeeds when the retried turn ends without an error", () => {
    const sent = sendingProviderRetry({
      reason: "server",
      attempt: 2,
      retryAt: 1,
    });
    expect(sent).toEqual({ reason: "server", attempt: 2, sending: true });
    expect(providerRetrySucceeded(session({ providerRetry: sent, busy: true }))).toBe(
      false,
    );
    expect(providerRetrySucceeded(session({ providerRetry: sent }))).toBe(true);
    expect(
      providerRetryDue(session({ providerRetry: sent }), Number.MAX_SAFE_INTEGER),
    ).toBe(false);
  });
});

describe("providerRetryStatus", () => {
  it("says when and which retry comes next", () => {
    const retry = { reason: "server" as const, attempt: 2, retryAt: 95_000 };
    expect(providerRetryStatus(retry, 0)).toBe("Retry 2 of 4 in 1m 35s");
    expect(providerRetryStatus(retry, 60_000)).toBe("Retry 2 of 4 in 35s");
    expect(providerRetryStatus(retry, 95_000)).toBe("Retry 2 of 4 now");
  });

  it("says why it won't retry by itself", () => {
    expect(
      providerRetryStatus({ reason: "server", attempt: PROVIDER_RETRY_MAX + 1 }, 0),
    ).toBe("Gave up after 4 retries");
    expect(providerRetryStatus({ reason: "server", attempt: 1 }, 0)).toBe(
      "Won't retry automatically",
    );
  });
});
