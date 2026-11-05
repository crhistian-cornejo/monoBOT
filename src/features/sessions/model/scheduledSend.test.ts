import { describe, expect, it } from "vitest";
import { canDispatchQueuedHead, queuedHead } from "./messageQueue";
import {
  formatScheduledSend,
  hasImmediateQueuedMessage,
  nextScheduledSendAt,
  releaseDueScheduledMessages,
  releaseScheduledMessages,
} from "./scheduledSend";
import { newSession, type QueuedMessage, type Session } from "./session";

const message = (id: string, sendAt?: number): QueuedMessage => ({
  id,
  text: id,
  attachments: [],
  ...(sendAt != null ? { sendAt } : {}),
});

function queued(messages: QueuedMessage[], patch: Partial<Session> = {}) {
  return {
    ...newSession("claude", "/tmp/project"),
    queuedMessages: messages,
    queueStatus: "active" as const,
    ...patch,
  };
}

describe("the queue passes over scheduled messages", () => {
  it("sends the first message that isn't waiting", () => {
    const session = queued([message("later", 5_000), message("now")]);
    expect(queuedHead(session)?.id).toBe("now");
    expect(canDispatchQueuedHead(session)).toBe(true);
  });

  it("has nothing to send while every message waits", () => {
    const session = queued([message("later", 5_000)]);
    expect(queuedHead(session)).toBeUndefined();
    expect(canDispatchQueuedHead(session)).toBe(false);
    expect(hasImmediateQueuedMessage(session)).toBe(false);
  });
});

describe("releaseDueScheduledMessages", () => {
  it("releases messages whose time came, in place", () => {
    const session = queued([
      message("first", 1_000),
      message("normal"),
      message("later", 9_000),
    ]);
    const released = releaseDueScheduledMessages(session, 1_000);
    expect(released.queuedMessages).toEqual([
      message("first"),
      message("normal"),
      message("later", 9_000),
    ]);
    expect(queuedHead(released)?.id).toBe("first");
  });

  it("returns the same session when nothing is due", () => {
    const session = queued([message("later", 9_000)]);
    expect(releaseDueScheduledMessages(session, 1_000)).toBe(session);
  });

  it("starts a queue that had no status, and keeps a paused one paused", () => {
    expect(
      releaseDueScheduledMessages(
        queued([message("a", 0)], { queueStatus: undefined }),
        1,
      ).queueStatus,
    ).toBe("active");
    expect(
      releaseDueScheduledMessages(
        queued([message("a", 0)], { queueStatus: "paused" }),
        1,
      ).queueStatus,
    ).toBe("paused");
  });

  it("sends one message now on request", () => {
    const session = queued([message("a", 9_000), message("b", 9_000)]);
    const released = releaseScheduledMessages(
      session,
      (entry) => entry.id === "b",
    );
    expect(released.queuedMessages?.map((entry) => entry.sendAt)).toEqual([
      9_000,
      undefined,
    ]);
  });
});

describe("nextScheduledSendAt", () => {
  it("finds the earliest scheduled send", () => {
    expect(
      nextScheduledSendAt(
        queued([message("a", 9_000), message("b"), message("c", 4_000)]),
      ),
    ).toBe(4_000);
    expect(nextScheduledSendAt(queued([message("b")]))).toBeUndefined();
  });
});

describe("formatScheduledSend", () => {
  it("names today, tomorrow and later days", () => {
    const now = new Date(2026, 10, 5, 14, 0).getTime();
    expect(formatScheduledSend(new Date(2026, 10, 5, 15, 30).getTime(), now)).not.toMatch(
      /Tomorrow|,/,
    );
    expect(
      formatScheduledSend(new Date(2026, 10, 6, 9, 0).getTime(), now),
    ).toMatch(/^Tomorrow /);
    expect(
      formatScheduledSend(new Date(2026, 10, 9, 9, 0).getTime(), now),
    ).toContain(",");
  });
});
