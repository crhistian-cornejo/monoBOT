import { describe, expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import {
  directMember,
  finishedGroupReplies,
  groupMemberPost,
  groupPostsContext,
  postedGroupRequests,
  unseenGroupPosts,
} from "./groupReplies";

const NOW = Date.UTC(2026, 10, 6);
const user = (id: string, text: string, extra: Partial<Block> = {}): Block => ({
  id,
  role: "user",
  text,
  startedAt: NOW,
  ...extra,
});
const said = (id: string, text: string, extra: Partial<Block> = {}): Block => ({
  id,
  role: "assistant",
  text,
  ...extra,
});
const look = { monoId: "docs", name: "Documents", mascot: "m", color: "c" };

describe("group members' answers", () => {
  it("takes the last written reply to each group message once it is done", () => {
    const member = {
      busy: false,
      blocks: [
        user("u1", "<group_message>…", { appRequestId: "app-g1-r1" }),
        said("a1", "Let me look"),
        said("t1", "", { tool: { name: "Read" } as Block["tool"] }),
        said("a2", "The PDF says 3 m/s"),
        user("u2", "something from the user"),
        said("a3", "Not for the group"),
        user("u3", "<group_message>…", { appRequestId: "app-g2-r9" }),
        said("a4", "Other group"),
      ],
    };
    expect(finishedGroupReplies(member, "g1")).toEqual([
      { request: "app-g1-r1", text: "The PDF says 3 m/s", at: NOW },
    ]);
  });

  it("waits for a running turn and leaves PASS, empty and old answers out", () => {
    const blocks = [
      user("u1", "q", { appRequestId: "app-g1-r1" }),
      said("a1", "Half an answer"),
    ];
    expect(finishedGroupReplies({ busy: true, blocks }, "g1")).toEqual([]);
    expect(finishedGroupReplies({ busy: false, blocks }, "g1")).toHaveLength(1);
    const pass = [user("u1", "q", { appRequestId: "app-g1-r1" }), said("a", "PASS")];
    expect(finishedGroupReplies({ busy: false, blocks: pass }, "g1")).toEqual([]);
    const old = [
      user("u1", "q", { appRequestId: "app-g1-r1", startedAt: 1 }),
      said("a", "Old"),
    ];
    expect(finishedGroupReplies({ busy: false, blocks: old }, "g1")).toEqual([]);
  });

  it("posts each request once, under the member's name", () => {
    const post = groupMemberPost(
      look,
      { request: "app-g1-r1", text: "Done", at: NOW },
      "p1",
      NOW,
    );
    expect(post).toMatchObject({
      role: "assistant",
      text: "Done",
      groupMember: { name: "Documents", request: "app-g1-r1" },
    });
    expect(postedGroupRequests({ blocks: [post] })).toEqual(new Set(["app-g1-r1"]));
  });
});

describe("@Name", () => {
  const members = [
    { id: "a", name: "Asistente General" },
    { id: "d", name: "Documents" },
  ];

  it("finds the one member a message opens with, spaces and case aside", () => {
    expect(directMember("@asistente general busca el correo", members)).toEqual({
      member: members[0],
      message: "busca el correo",
    });
    expect(directMember("@Documents, resume el PDF", members)?.member.id).toBe("d");
  });

  it("leaves to the lead what names several, no one, or only part of a name", () => {
    expect(directMember("@Documents y @Asistente General, comparen", members)).toBeUndefined();
    expect(directMember("hola @Documents", members)).toBeUndefined();
    expect(directMember("@Documentsx hola", members)).toBeUndefined();
    expect(directMember("@Documents", members)).toBeUndefined();
  });

  it("tells the lead what went around it since the user last wrote to it", () => {
    const blocks: Block[] = [
      user("u0", "Hola"),
      said("l0", "Hola"),
      user("u1", "@Documents resume el PDF", {
        appRequestId: "group-direct-k1",
      }),
      groupMemberPost(
        look,
        { request: "app-g1-direct-k1", text: "Resumen: 3 m/s", at: NOW },
        "p1",
      ),
    ];
    const exchanges = unseenGroupPosts(blocks);
    expect(exchanges).toHaveLength(1);
    const context = groupPostsContext(exchanges);
    expect(context).toContain('The user: "@Documents resume el PDF"');
    expect(context).toContain("Documents: Resumen: 3 m/s");
    expect(groupPostsContext([])).toBe("");
  });
});
