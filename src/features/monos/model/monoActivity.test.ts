import { expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  latestMonoActivity,
  resolveMonoActivity,
  type MonoActivitySelection,
} from "./monoActivity";

const blocks: Block[] = [
  { id: "user", role: "user", text: "Inspect" },
  {
    id: "call",
    role: "tool",
    text: "Inspect file",
    tool: { status: "in_progress" },
  },
];
const selection: MonoActivitySelection = {
  sessionId: "mono",
  turnId: "user",
  blocks,
};

it("follows the selected live turn, including new steps and settled tool state", () => {
  const updated: Block[] = [
    blocks[0],
    { ...blocks[1], tool: { status: "completed" } },
    { id: "reply", role: "assistant", text: "Done" },
  ];
  expect(
    resolveMonoActivity(selection, { id: "mono", blocks: updated, busy: true }),
  ).toEqual({
    turnId: "user",
    blocks: updated,
    live: true,
  });
  expect(
    resolveMonoActivity(selection, { id: "mono", blocks: updated, busy: false })
      ?.live,
  ).toBe(false);
  expect(
    resolveMonoActivity(selection, {
      id: "mono",
      blocks: [
        ...updated,
        { id: "next", role: "user", text: "Another request" },
      ],
      busy: true,
    })?.live,
  ).toBe(false);
});

it("retains an archived turn without borrowing another turn or Mono's activity", () => {
  expect(
    resolveMonoActivity(selection, {
      id: "mono",
      blocks: [{ id: "new", role: "user", text: "Latest" }],
      busy: true,
    }),
  ).toEqual({
    turnId: "user",
    blocks,
    live: false,
  });
  expect(
    resolveMonoActivity(selection, { id: "another-mono", blocks }),
  ).toBeNull();
  expect(resolveMonoActivity(selection, undefined)).toBeNull();
  expect(resolveMonoActivity(null, { id: "mono", blocks })).toBeNull();
});

it("keeps completion activity separate from the previous reply before any output arrives", () => {
  const notification: Block = {
    id: "notification",
    role: "user",
    text: "Hidden completion prompt",
    internal: true,
    startedAt: 2_000,
    monoSessionCompletion: {
      sessionId: "worker",
      title: "Review",
      status: "completed",
    },
  };
  const session = { id: "mono", blocks: [...blocks, notification], busy: true };
  expect(resolveMonoActivity(selection, session)?.live).toBe(false);
  const completionSelection = {
    sessionId: "mono",
    turnId: notification.id,
    blocks: [notification],
  };
  const step: Block = {
    id: "check-report",
    role: "tool",
    text: "Read the review session",
    tool: { status: "in_progress" },
  };
  const updated = { ...session, blocks: [...session.blocks, step] };
  expect(resolveMonoActivity(completionSelection, updated)).toEqual({
    turnId: notification.id,
    blocks: [notification, step],
    live: true,
  });
});

it("follows the running turn, or the last one that did work, with none picked", () => {
  const reply: Block = { id: "reply", role: "assistant", text: "Done" };
  const chat: Block = { id: "chat", role: "user", text: "Thanks" };
  const thanks: Block = { id: "thanks", role: "assistant", text: "Anytime" };
  expect(latestMonoActivity(undefined)).toBeNull();
  expect(
    latestMonoActivity({ id: "mono", blocks: [...blocks, reply], busy: true }),
  ).toEqual({ turnId: "user", blocks: [...blocks, reply], live: true });
  // A settled chat turn without tools leaves the last working turn in view.
  expect(
    latestMonoActivity({
      id: "mono",
      blocks: [...blocks, reply, chat, thanks],
      busy: false,
    })?.turnId,
  ).toBe("user");
  // A new request shows at once, before its first tool call.
  expect(
    latestMonoActivity({
      id: "mono",
      blocks: [...blocks, reply, chat],
      busy: true,
    }),
  ).toMatchObject({ turnId: "chat", live: true });
  expect(
    latestMonoActivity({ id: "mono", blocks: [chat, thanks], busy: false }),
  ).toBeNull();
});
