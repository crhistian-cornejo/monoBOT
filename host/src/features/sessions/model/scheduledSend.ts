import type { QueuedMessage, Session } from "./session";

/**
 * "Send later" offers the same times as a session's "Remind me"
 * (`sessionReminderPresets`), plus this step to pick any date and time.
 */
export const SEND_LATER_CUSTOM = "send-later:custom";

/** "15:30" today, "Tomorrow 9:00", "Fri, Nov 7, 9:00" later. */
export function formatScheduledSend(sendAt: number, now: number): string {
  const date = new Date(sendAt);
  const today = new Date(now);
  const tomorrow = new Date(now);
  tomorrow.setDate(tomorrow.getDate() + 1);
  const time = date.toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
  if (date.toDateString() === today.toDateString()) return time;
  if (date.toDateString() === tomorrow.toDateString()) return `Tomorrow ${time}`;
  return `${date.toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })}, ${time}`;
}

export function isScheduled(message: QueuedMessage): boolean {
  return message.sendAt != null;
}

/** A message the queue would send as soon as the session allows. */
export function hasImmediateQueuedMessage(session: Session): boolean {
  return !!session.queuedMessages?.some((message) => !isScheduled(message));
}

/** The earliest scheduled send, to know when to look again. */
export function nextScheduledSendAt(session: Session): number | undefined {
  let next: number | undefined;
  for (const message of session.queuedMessages ?? []) {
    if (message.sendAt != null && (next == null || message.sendAt < next))
      next = message.sendAt;
  }
  return next;
}

/**
 * Turn scheduled messages into ordinary queued ones: those whose time has come,
 * or the one the user sends now. They keep their place in the queue.
 */
export function releaseScheduledMessages(
  session: Session,
  release: (message: QueuedMessage) => boolean,
): Session {
  let changed = false;
  const queuedMessages = session.queuedMessages?.map((message) => {
    if (!isScheduled(message) || !release(message)) return message;
    changed = true;
    const { sendAt: _sent, ...rest } = message;
    return rest;
  });
  if (!changed) return session;
  return {
    ...session,
    queuedMessages,
    queueStatus: session.queueStatus ?? "active",
  };
}

export function releaseDueScheduledMessages(
  session: Session,
  now: number,
): Session {
  return releaseScheduledMessages(
    session,
    (message) => message.sendAt! <= now,
  );
}
