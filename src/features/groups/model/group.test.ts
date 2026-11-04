import { describe, expect, it } from "vitest";
import { GROUP_MAX_MEMBERS, groupMessage, parseGroup } from "./group";
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
  });

  it("tells the leader how to run the room", () => {
    const brief = groupBrief("Trip", { members: [], goal: "Plan the trip" });
    expect(brief).toContain('You lead the group "Trip", which is for: Plan the trip');
    expect(brief).toContain("app group.ask");
    expect(brief).toContain("@Name");
    expect(brief).toContain("at most 3 rounds and 10 member messages");
    expect(brief).toContain("no members yet");
  });
});
