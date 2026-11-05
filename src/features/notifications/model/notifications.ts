import { invoke } from "@tauri-apps/api/core";
import { HARNESS_TITLE, sessionDisplayTitle, type Session } from "../../sessions/model/session";
import { loadSoundsEnabled, playCue } from "../../settings/model/sounds";
import {
  allowsProjectNotification,
  type NotificationSubject,
} from "./notificationPreferences";
import { monoReplyText } from "../../monos/model/monoReaction";
import { newsPreview } from "../../bots/model/botNews";
import { monoForSession, monoLook } from "../../monos/model/mono";
import {
  knownNotificationProject,
  type NotificationProject,
} from "./notificationProjects";

/**
 * Agents (Monos and bots) chat from the home folder, which is no project, so
 * their approvals and questions would never alert. They alert under this one
 * entry instead, which the user can mute as a whole.
 */
export const AGENTS_NOTIFICATION_PROJECT: NotificationProject = {
  id: "agents",
  name: "Agents",
  detail: "Your Monos and bots",
  kind: "local",
  paths: [],
};

/** The project a session's alerts belong to, if it has one. */
export function notificationProjectFor(
  session: Pick<Session, "id" | "cwd">,
): NotificationProject | undefined {
  return (
    knownNotificationProject(session.cwd) ??
    (monoForSession(session.id) ? AGENTS_NOTIFICATION_PROJECT : undefined)
  );
}

const KEY = "monocode.notifications";

/** Off until the user opts in; enabling asks the OS for permission. */
export const NOTIFICATIONS_DEFAULT = false;

export const NOTIFICATIONS_CHANGE_EVENT = "monocode:notifications-change";

/** Rust emits this with the session id when a notification is clicked. */
export const NOTIFICATION_CLICK_EVENT = "monocode:notification-click";

export type NotificationPermission =
  "prompt" | "granted" | "denied" | "unsupported";

export function loadNotificationsEnabled(): boolean {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw == null) return NOTIFICATIONS_DEFAULT;
    return raw === "1" || raw === "true";
  } catch {
    return NOTIFICATIONS_DEFAULT;
  }
}

export function saveNotificationsEnabled(value: boolean) {
  try {
    localStorage.setItem(KEY, value ? "1" : "0");
  } catch {
    // private mode / quota
  }
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent<boolean>(NOTIFICATIONS_CHANGE_EVENT, { detail: value }),
  );
}

let permission: NotificationPermission = "prompt";

/** Last permission the OS reported; refreshed by the probes below. */
export function cachedNotificationPermission(): NotificationPermission {
  return permission;
}

export async function probeNotificationPermission(): Promise<NotificationPermission> {
  try {
    permission = await invoke<NotificationPermission>(
      "notification_permission",
    );
  } catch {
    permission = "unsupported";
  }
  return permission;
}

/** Shows the OS prompt when undecided; otherwise reports the current state. */
export async function requestNotificationPermission(): Promise<NotificationPermission> {
  try {
    permission = await invoke<NotificationPermission>(
      "request_notification_permission",
    );
  } catch {
    permission = "unsupported";
  }
  return permission;
}

export function openNotificationSettings(): Promise<void> {
  return invoke<void>("open_notification_settings");
}

/**
 * Tracked from Tauri's focus event rather than `document.hasFocus()`, which
 * WKWebView keeps reporting true after the window drops to the background.
 */
let windowFocused =
  typeof document !== "undefined" ? document.hasFocus() : true;

export function setWindowFocused(focused: boolean) {
  windowFocused = focused;
}

/**
 * A banner only earns its place while the user is looking elsewhere: another
 * app, or another session. The transcript already shows the change on the
 * session that is on screen.
 */
export function shouldNotify({
  enabled,
  permission,
  windowFocused,
  sessionVisible,
}: {
  enabled: boolean;
  permission: NotificationPermission;
  windowFocused: boolean;
  sessionVisible: boolean;
}): boolean {
  if (!enabled || (windowFocused && sessionVisible)) return false;
  return permission === "granted" || permission === "prompt";
}

export type InputNotificationEvent = {
  kind: "approval" | "question";
  requestId: number;
};
export type NotificationEvent = "finished" | InputNotificationEvent;

type PendingInputNotification = {
  session: Session;
  event: InputNotificationEvent;
};

/** Track each request, including a new request in an already-waiting session. */
export function pendingInputNotifications(
  sessions: Session[],
): Map<string, PendingInputNotification> {
  const pending = new Map<string, PendingInputNotification>();
  for (const session of sessions) {
    if (session.inboxAsk) continue;
    for (const block of session.blocks) {
      if (block.approval && !block.approval.decided) {
        pending.set(
          JSON.stringify([session.id, "approval", block.approval.requestId]),
          {
            session,
            event: { kind: "approval", requestId: block.approval.requestId },
          },
        );
      }
    }
    if (session.pendingQuestion) {
      pending.set(
        JSON.stringify([
          session.id,
          "question",
          session.pendingQuestion.requestId,
        ]),
        {
          session,
          event: {
            kind: "question",
            requestId: session.pendingQuestion.requestId,
          },
        },
      );
    }
  }
  return pending;
}

/** App name, then the session title, then the reply itself. */
export type NotificationText = {
  title: string;
  subtitle: string;
  body: string;
};

const BODY_MAX = 240;

export function notificationText(
  session: Session,
  event: NotificationEvent,
): NotificationText {
  const title = "MonoBOT";
  // An agent's chat is named after the agent, as the rail shows it.
  const agent = monoForSession(session.id);
  const subtitle = agent
    ? monoLook(agent).name
    : sessionDisplayTitle(session.title, session.harness);
  const harness = HARNESS_TITLE[session.harness];
  if (event !== "finished") {
    if (event.kind === "question") {
      const question =
        session.pendingQuestion?.requestId === event.requestId
          ? session.pendingQuestion
          : undefined;
      const prompt = question?.title || question?.questions[0]?.prompt;
      return {
        title,
        subtitle,
        body: clip(prompt || `${harness} has a question for you`),
      };
    }
    const pending = session.blocks.find(
      (block) =>
        block.approval?.requestId === event.requestId &&
        !block.approval.decided,
    );
    const what = pending?.tool?.title || pending?.text;
    return {
      title,
      subtitle,
      body: clip(what ? `Approve: ${what}` : `${harness} needs your approval`),
    };
  }
  const reply = [...session.blocks]
    .reverse()
    .find((block) => block.role === "assistant" && block.text.trim());
  return {
    title,
    subtitle,
    body: clip((reply && monoReplyText(reply.text)) || `${harness} finished`),
  };
}

/** First paragraph, whitespace collapsed; macOS wraps and truncates the rest. */
function clip(text: string): string {
  const paragraph =
    text
      .split(/\n\s*\n/)
      .map((part) => part.replace(/\s+/g, " ").trim())
      .find((part) => part.length > 0) ?? "";
  return paragraph.length > BODY_MAX
    ? `${paragraph.slice(0, BODY_MAX - 1)}…`
    : paragraph;
}

/**
 * Sends the banner when policy allows. Resolves true once the OS accepted it
 * so callers can skip the in-app cue: the OS sound stands in for it. A
 * rejected dispatch resolves false so the cue still plays.
 */
export async function notifySession(
  session: Session,
  event: NotificationEvent,
  sessionVisible: boolean,
): Promise<boolean> {
  if (session.inboxAsk) return false;
  const occurredAt = Date.now();
  const project = notificationProjectFor(session);
  if (!project) return false;
  return notifyProjectSession(session, event, sessionVisible, {
    projectId: project.id,
    category: event === "finished" ? "agentFinished" : "agentInput",
    occurredAt,
  });
}

/**
 * A personal bot posted on its own. Its chat runs in the home folder, not a
 * project, so this skips the per-project rules that would otherwise drop it.
 */
export async function announceBotPost(
  sessionId: string,
  botName: string,
  source: string,
  text: string,
  sessionVisible: boolean,
): Promise<void> {
  const decision = shouldNotify({
    enabled: loadNotificationsEnabled(),
    permission,
    windowFocused,
    sessionVisible,
  });
  if (decision) {
    try {
      await invoke("show_notification", {
        sessionId,
        title: botName,
        subtitle: source,
        body: newsPreview(text),
        sound: loadSoundsEnabled(),
      });
      return;
    } catch {
      // fall through to the in-app cue
    }
  }
  playCue("turnFinished", { projectId: "", category: "agentFinished", occurredAt: Date.now() });
}

export function announceSessionFinished(
  session: Session,
  sessionVisible: boolean,
): Promise<void> {
  return announceSession(session, "finished", sessionVisible);
}

export function announceSessionInput(
  session: Session,
  event: InputNotificationEvent,
  sessionVisible: boolean,
): Promise<void> {
  return announceSession(session, event, sessionVisible);
}

/** One policy decision covers both the OS banner and its in-app sound fallback. */
async function announceSession(
  session: Session,
  event: NotificationEvent,
  sessionVisible: boolean,
): Promise<void> {
  if (session.inboxAsk) return;
  const occurredAt = Date.now();
  const project = notificationProjectFor(session);
  if (!project) return;
  const finished = event === "finished";
  const subject: NotificationSubject = {
    projectId: project.id,
    category: finished ? "agentFinished" : "agentInput",
    occurredAt,
  };
  const sent = await notifyProjectSession(
    session,
    event,
    sessionVisible,
    subject,
  );
  if (!sent) playCue(finished ? "turnFinished" : "inputRequested", subject);
}

async function notifyProjectSession(
  session: Session,
  event: NotificationEvent,
  sessionVisible: boolean,
  subject: NotificationSubject,
): Promise<boolean> {
  if (!allowsProjectNotification(subject)) return false;
  const decision = shouldNotify({
    enabled: loadNotificationsEnabled(),
    permission,
    windowFocused,
    sessionVisible,
  });
  if (!decision) return false;
  const { title, subtitle, body } = notificationText(session, event);
  try {
    await invoke("show_notification", {
      sessionId: session.id,
      title,
      subtitle,
      body,
      sound: loadSoundsEnabled(),
    });
    return true;
  } catch {
    return false;
  }
}
