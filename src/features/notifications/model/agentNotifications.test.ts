// @vitest-environment happy-dom
import { beforeEach, expect, it } from "vitest";
import { createMono, saveMonoSessionId } from "../../monos/model/mono";
import { newSession } from "../../sessions/model/session";
import {
  AGENTS_NOTIFICATION_PROJECT,
  notificationProjectFor,
  notificationText,
} from "./notifications";

beforeEach(() => localStorage.clear());

it("alerts for an agent working in the home folder, under Agents", () => {
  const bot = createMono([], {
    role: "assistant",
    name: "IVY",
    instructions: "",
  });
  const session = newSession("claude", "/Users/me", "claude:sonnet");
  saveMonoSessionId(bot.id, session.id);
  // Home is not a project, so before this an agent's approval never alerted.
  expect(notificationProjectFor(session)).toEqual(AGENTS_NOTIFICATION_PROJECT);
  const pending = {
    ...session,
    blocks: [
      {
        id: "1",
        role: "tool" as const,
        text: "",
        tool: { title: "Read /tmp/report.pdf" },
        approval: { requestId: 1 },
      },
    ],
  };
  expect(notificationText(pending, { kind: "approval", requestId: 1 })).toEqual(
    {
      title: "MonoBOT",
      subtitle: "IVY",
      body: "Approve: Read /tmp/report.pdf",
    },
  );
  // Any other chat in the home folder still has no project to alert under.
  expect(
    notificationProjectFor(newSession("claude", "/Users/me", "claude:sonnet")),
  ).toBeUndefined();
});
