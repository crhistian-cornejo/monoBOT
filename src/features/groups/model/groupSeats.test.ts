// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { monoForSession, type Mono } from "../../monos/model/mono";
import { parseGroup } from "./group";
import {
  isGroupSeat,
  saveSeat,
  seatBrief,
  seatOf,
  seatSessionId,
} from "./groupSeats";

const roster: Mono[] = [
  {
    id: "docs",
    sessionId: "docs-chat",
    name: "Documents",
    mascot: "invader",
    color: "green",
    projects: [],
    kind: "bot",
    botRole: "documents",
  },
  {
    id: "nexa",
    sessionId: "nexa-chat",
    name: "Nexa",
    mascot: "mushroom",
    color: "purple",
    projects: [],
    kind: "group",
    group: { members: ["docs"] },
  },
];

describe("a member's seat in a group", () => {
  beforeEach(() => {
    localStorage.clear();
    localStorage.setItem("monocode:mono-roster", JSON.stringify(roster));
  });

  it("is the same agent with a conversation of its own", () => {
    expect(isGroupSeat("docs-seat")).toBe(false);
    saveSeat("nexa", "docs", "docs-seat");
    expect(seatSessionId("nexa", "docs")).toBe("docs-seat");
    expect(seatOf("docs-seat")?.group.id).toBe("nexa");
    expect(seatOf("docs-seat")?.member.id).toBe("docs");
    // The seat acts as the member; its own chat is still its own.
    expect(monoForSession("docs-seat")?.id).toBe("docs");
    expect(monoForSession("docs-chat")?.id).toBe("docs");
    expect(seatOf("docs-chat")).toBeUndefined();
  });

  it("keeps only seats of current members", () => {
    expect(
      parseGroup({
        members: ["docs"],
        seats: { docs: "docs-seat", gone: "x", bad: 4 },
      })?.seats,
    ).toEqual({ docs: "docs-seat" });
  });

  it("tells the member where its words go", () => {
    const brief = seatBrief("Nexa", "Documents");
    expect(brief).toContain('own seat in the group "Nexa"');
    expect(brief).toContain('"personal":true');
  });
});
