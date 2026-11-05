import type {
  Block,
  GroupMemberPost,
  Session,
} from "../../sessions/model/session";

/**
 * Members' answers post in the group's chat from this release on; older
 * group.ask exchanges stay in the members' own chats, so opening a group
 * does not replay its whole history.
 */
export const GROUP_POSTS_SINCE = Date.UTC(2026, 10, 5, 18, 0);

const REPLY_MAX_CHARS = 20_000;

/** The receipt prefix of every message a group's chat sends a member. */
export function groupRequestPrefix(groupSessionId: string): string {
  return `app-${groupSessionId}-`;
}

/**
 * The member's turn after the message at `start`: its last written reply,
 * the error it stopped on, and whether a later message closed it.
 */
export function turnAfter(
  blocks: readonly Block[],
  start: number,
): { text: string; error?: string; finished: boolean } {
  let text = "";
  let error: string | undefined;
  for (const block of blocks.slice(start + 1)) {
    if (block.role === "user" && !block.internal)
      return { text, ...(error ? { error } : {}), finished: true };
    if (
      block.role === "assistant" &&
      !block.tool &&
      !block.internal &&
      block.text.trim()
    )
      text = block.text.trim();
    if (block.notice === "error" && block.text.trim())
      error = block.text.trim();
  }
  return { text, ...(error ? { error } : {}), finished: false };
}

export type GroupReply = { request: string; text: string; at: number };

/**
 * The answers a member has finished for a group's chat: the last written
 * reply after each message the group sent it. A turn still running is not
 * finished; neither is a PASS, an empty answer or one sent before posts.
 */
export function finishedGroupReplies(
  member: Pick<Session, "blocks" | "busy">,
  groupSessionId: string,
  since = GROUP_POSTS_SINCE,
): GroupReply[] {
  const prefix = groupRequestPrefix(groupSessionId);
  const replies: GroupReply[] = [];
  const blocks = member.blocks;
  for (let index = 0; index < blocks.length; index++) {
    const ask = blocks[index];
    if (ask.role !== "user" || !ask.appRequestId?.startsWith(prefix)) continue;
    if (isDroppedGroupRequest(ask.appRequestId)) continue;
    const turn = turnAfter(blocks, index);
    if (!turn.finished && member.busy) continue;
    const at = ask.startedAt ?? ask.sentAt;
    // A member that stopped on an error says so in the group, not silence.
    const answer =
      turn.text || (turn.error ? `Couldn't answer: ${turn.error}` : "");
    if (at == null || at < since || !answer || answer === "PASS") continue;
    replies.push({
      request: ask.appRequestId,
      text: answer.slice(0, REPLY_MAX_CHARS),
      at,
    });
  }
  return replies;
}

/** The requests whose answers the group's chat already shows. */
export function postedGroupRequests(group: Pick<Session, "blocks">): Set<string> {
  return new Set(
    group.blocks.flatMap((block) =>
      block.groupMember ? [block.groupMember.request] : [],
    ),
  );
}

/** A member's answer as the group's chat shows it. */
export function groupMemberPost(
  member: Omit<GroupMemberPost, "request" | "at">,
  reply: GroupReply,
  id: string,
  now = Date.now(),
): Block {
  return {
    id,
    role: "assistant",
    text: reply.text,
    groupMember: { ...member, request: reply.request, at: now },
  };
}

/** The receipt of a message the user sent one member with @Name. */
export const DIRECT_PREFIX = "group-direct-";
/** The receipt of a message every member answered before the lead. */
export const ROUND_PREFIX = "group-round-";
/** The receipt of an errand the first member to do it answered. */
export const QUICK_PREFIX = "group-quick-";

/**
 * Whether a group chat's block is a message the members answered around
 * the lead: the user's @Name to one member, or an errand.
 */
export function isDirectAsk(block: Block): boolean {
  return (
    block.role === "user" &&
    (!!block.appRequestId?.startsWith(DIRECT_PREFIX) ||
      !!block.appRequestId?.startsWith(QUICK_PREFIX))
  );
}

/** Answers to errands that another member did first. */
const DROPPED_KEY = "monocode.groupRequests.dropped";
const DROPPED_MAX = 300;

function droppedRequests(): string[] {
  try {
    const parsed: unknown = JSON.parse(localStorage.getItem(DROPPED_KEY) ?? "[]");
    return Array.isArray(parsed)
      ? parsed.filter((entry): entry is string => typeof entry === "string")
      : [];
  } catch {
    return [];
  }
}

/**
 * Keeps a member's part of an errand out of the group's chat: another
 * member did it first, and this one was stopped mid-way.
 */
export function dropGroupRequests(requests: readonly string[]) {
  if (!requests.length) return;
  const next = [...new Set([...requests, ...droppedRequests()])].slice(
    0,
    DROPPED_MAX,
  );
  try {
    localStorage.setItem(DROPPED_KEY, JSON.stringify(next));
  } catch {
    // private mode / quota: kept out for this window only
  }
  memoryDropped = new Set(next);
}

let memoryDropped: Set<string> | undefined;

export function isDroppedGroupRequest(request: string): boolean {
  memoryDropped ??= new Set(droppedRequests());
  return memoryDropped.has(request);
}

/**
 * The member a message opens with @Name for, and what to send it. Only one
 * member: a message for several, or for the lead, goes to the lead, who
 * splits it. Names may hold spaces, so the longest match wins.
 */
export function directMember<T extends { id: string; name: string }>(
  text: string,
  members: readonly T[],
): { member: T; message: string } | undefined {
  const trimmed = text.trimStart();
  if (!trimmed.startsWith("@")) return undefined;
  const lower = trimmed.toLowerCase();
  const member = [...members]
    .filter((entry) => entry.name.trim())
    .sort((a, b) => b.name.length - a.name.length)
    .find((entry) => {
      const name = `@${entry.name.trim().toLowerCase()}`;
      const next = lower[name.length] ?? "";
      return lower.startsWith(name) && !/[\p{L}\p{N}_]/u.test(next);
    });
  if (!member) return undefined;
  const message = trimmed.slice(member.name.trim().length + 1).trim();
  // "@Documents and @Asistente…" is for the lead to split.
  const others = members.some(
    (entry) =>
      entry !== member &&
      !!entry.name.trim() &&
      message.toLowerCase().includes(`@${entry.name.trim().toLowerCase()}`),
  );
  if (!message || others) return undefined;
  return { member, message };
}

type DirectExchange = { ask?: string; post: Block };

/**
 * What went around the lead in its chat since the user last wrote to it:
 * the @Name messages and the members' answers to them. It saw the answers
 * to its own group.ask; these it reads with the user's next message.
 */
export function unseenGroupPosts(blocks: readonly Block[]): DirectExchange[] {
  const exchanges: DirectExchange[] = [];
  let posts: Block[] = [];
  for (let index = blocks.length - 1; index >= 0; index--) {
    const block = blocks[index];
    if (isDirectAsk(block)) {
      for (const post of posts) exchanges.unshift({ ask: block.text, post });
      posts = [];
      continue;
    }
    if (block.role === "user" && !block.draft && !block.internal) break;
    const request = block.groupMember?.request;
    if (request?.includes("-direct-") || request?.includes("-quick-"))
      posts.unshift(block);
  }
  for (const post of posts) exchanges.unshift({ post });
  return exchanges;
}

const clip = (text: string, max: number) => {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > max ? `${flat.slice(0, max - 1)}…` : flat;
};

/** Tells the lead what the user and members said around it in its chat. */
export function groupPostsContext(exchanges: readonly DirectExchange[]): string {
  if (!exchanges.length) return "";
  const lines = exchanges.map(({ ask, post }) => {
    const asked = ask ? `The user: "${clip(ask, 300)}" → ` : "";
    return `- ${asked}${post.groupMember!.name}: ${clip(post.text, 1200)}`;
  });
  return `<group_posts>\nThe user wrote to these members directly (with @Name, or an errand the first member able to did), and they answered in this chat; the user has read them. Build on them rather than repeating them, and save to memory what they decided.\n${lines.join("\n")}\n</group_posts>`;
}
