import { describe, expect, it } from "vitest";
import {
  GROUP_MAX_MEMBERS,
  accessAnswerText,
  answerAccessRequest,
  groupAccessFor,
  groupMessage,
  isInside,
  memberPlaces,
  parseGroup,
  withAccessRequest,
  type Group,
} from "./group";
import { groupBrief } from "./groupBrief";

describe("groups", () => {
  it("keeps unique member ids up to the limit", () => {
    const ids = Array.from({ length: 12 }, (_, index) => `m${index}`);
    const group = parseGroup({ members: [...ids, "m1", 4, ""], goal: "  Plan  " });
    expect(group?.members).toEqual(ids.slice(0, GROUP_MAX_MEMBERS));
    expect(group?.goal).toBe("Plan");
    expect(parseGroup(null)).toBeUndefined();
    expect(parseGroup({})).toEqual({ members: [] });
  });

  it("frames what a member is asked and lets it pass", () => {
    const message = groupMessage("Trip", "Plan the trip to Cusco", " Find flights ");
    expect(message).toContain('<group_message group="Trip">');
    expect(message).toContain("The group is for: Plan the trip to Cusco");
    expect(message).toContain("Find flights");
    expect(message).toContain("reply with exactly PASS");
    expect(message).toContain('"wait":true');
  });

  it("tells the leader how to run the room", () => {
    const brief = groupBrief("Trip", { members: [], goal: "Plan the trip" });
    expect(brief).toContain('You lead the group "Trip", which is for: Plan the trip');
    expect(brief).toContain("app group.ask");
    expect(brief).toContain("@Name");
    expect(brief).toContain("at most 3 rounds and 10 member messages");
    expect(brief).toContain("no members yet");
    expect(brief).toContain("group.access");
    expect(brief).toContain("No folders are open to the members.");
  });
});

describe("a group's project", () => {
  const group: Group = {
    members: ["a"],
    folders: [
      { path: "D:\\Projects\\1064", edit: false },
      { path: "D:\\Projects\\1064\\Editables", edit: true },
    ],
    context: ["C:/Users/me/brief.pdf"],
  };

  it("keeps folders, context and answers it can trust", () => {
    const parsed = parseGroup({
      members: [],
      folders: [
        { path: "D:\\Docs", edit: true },
        { path: "d:/docs/", edit: false },
        { path: "relative", edit: true },
        { path: "C:\\Windows\\System32", edit: true },
      ],
      context: ["/home/me/a.pdf", "notes.txt"],
      answers: [
        { target: "https://example.com/a", access: "read", at: 1 },
        { target: "/srv/x", access: "maybe", at: 1 },
      ],
      requests: [{ id: "r1", target: "/srv/y", by: "Research", at: 2 }],
    });
    expect(parsed?.folders).toEqual([
      { path: "D:\\Docs", edit: true },
      { path: "C:\\Windows\\System32", edit: false },
    ]);
    expect(parsed?.context).toEqual(["/home/me/a.pdf"]);
    expect(parsed?.answers).toEqual([
      { target: "https://example.com/a", access: "read", at: 1 },
    ]);
    expect(parsed?.requests).toEqual([
      { id: "r1", target: "/srv/y", by: "Research", at: 2 },
    ]);
  });

  it("compares paths the way Windows does", () => {
    expect(isInside("d:/projects/1064/planos/a.pdf", "D:\\Projects\\1064")).toBe(true);
    expect(isInside("D:\\Projects\\10645", "D:\\Projects\\1064")).toBe(false);
    expect(isInside("/Users/me/Docs", "/users/me/docs")).toBe(false);
  });

  it("answers from the closest folder, the context and saved answers", () => {
    expect(groupAccessFor(group, "D:\\Projects\\1064\\Planos\\p1.dwg")).toBe("read");
    expect(groupAccessFor(group, "D:/Projects/1064/Editables/cap3.docx")).toBe("edit");
    expect(groupAccessFor(group, "C:\\Users\\me\\brief.pdf")).toBe("read");
    expect(groupAccessFor(group, "E:\\Other")).toBeUndefined();
    const answered = {
      ...group,
      answers: [
        { target: "E:\\Other", access: "no" as const, at: 1 },
        { target: "https://site.test/page/", access: "read" as const, at: 1 },
      ],
    };
    expect(groupAccessFor(answered, "E:\\Other\\x.txt")).toBe("no");
    expect(groupAccessFor(answered, "https://site.test/page#top")).toBe("read");
    expect(groupAccessFor(answered, "https://site.test/other")).toBeUndefined();
  });

  it("asks once per target and saves the user's answer", () => {
    const first = withAccessRequest(group, {
      id: "r1",
      target: "E:\\Survey",
      by: "Documents",
      at: 1,
    });
    const again = withAccessRequest(first.group, {
      id: "r2",
      target: "e:/survey/",
      at: 2,
    });
    expect(again.request.id).toBe("r1");
    expect(again.group.requests).toHaveLength(1);

    const granted = answerAccessRequest(again.group, "r1", "edit", 5);
    expect(granted.requests).toBeUndefined();
    expect(granted.folders).toContainEqual({ path: "E:\\Survey", edit: true });
    expect(granted.answers).toEqual([
      { target: "E:\\Survey", access: "edit", at: 5 },
    ]);
    expect(groupAccessFor(granted, "E:\\Survey\\data.xlsx")).toBe("edit");

    const refused = answerAccessRequest(first.group, "r1", "no", 6);
    expect(refused.folders).toEqual(group.folders);
    expect(groupAccessFor(refused, "E:\\Survey")).toBe("no");
    expect(answerAccessRequest(group, "missing", "read")).toBe(group);
  });

  it("says the answer in words the group chat acts on", () => {
    expect(accessAnswerText("E:\\Survey", "read")).toContain("read only");
    expect(accessAnswerText("E:\\Survey", "edit")).toContain("never delete");
    expect(accessAnswerText("https://x.test", "no")).toContain("this link");
  });

  it("gives a member each folder once, with the widest access", () => {
    const places = memberPlaces([
      group,
      { members: [], folders: [{ path: "d:/projects/1064", edit: true }] },
    ]);
    expect(places.folders).toEqual([
      { path: "D:\\Projects\\1064", edit: true },
      { path: "D:\\Projects\\1064\\Editables", edit: true },
    ]);
    expect(places.context).toEqual(["C:/Users/me/brief.pdf"]);
  });

  it("lists the folders and the access rule to the members", () => {
    const message = groupMessage("Nexa", undefined, "Review chapter 3", group);
    expect(message).toContain("D:\\Projects\\1064 (read only)");
    expect(message).toContain("Editables (read and edit, never delete)");
    expect(message).toContain("C:/Users/me/brief.pdf");
    expect(message).toContain("app group.access");
  });
});
