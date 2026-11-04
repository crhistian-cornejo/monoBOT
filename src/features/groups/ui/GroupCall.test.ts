import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { Mono } from "../../monos/model/mono";

const ANA = { id: "ana", mascot: "ghost", color: "#f00", projects: [] };
vi.mock("../../monos/model/mono", async (original) => ({
  ...(await original<object>()),
  findMono: (id: string) => (id === ANA.id ? ANA : undefined),
  monoLook: (mono: { id: string }) => ({
    name: mono.id === "ana" ? "Ana" : "Trip",
    mascot: "ghost",
    color: "#f00",
    projects: [],
  }),
  subscribeMonos: () => () => {},
  monosSnapshot: () => 1,
}));

const { GroupCall } = await import("./GroupCall");

const group = (members: string[]) =>
  ({
    id: "trip",
    kind: "group",
    mascot: "ghost",
    color: "#0f0",
    projects: [],
    group: { members, goal: "Plan the trip" },
  }) as unknown as Mono;

const render = (members: string[]) =>
  renderToStaticMarkup(
    createElement(GroupCall, {
      group: group(members),
      sessions: [],
      onAskMember: vi.fn(),
    }),
  );

describe("the Call tab", () => {
  it("shows who is on the call and offers to start it", () => {
    const html = render(["ana"]);
    expect(html).toContain('data-group-call="idle"');
    expect(html).toContain("Ana");
    expect(html).toContain("Start call");
    expect(html).toContain("name someone to hear just them");
    // No mic or composer until the call starts.
    expect(html).not.toContain("Hang up");
    expect(html).not.toContain("<textarea");
  });

  it("cannot start a call with no members", () => {
    const html = render([]);
    expect(html).toContain("Add members on the Members tab");
    expect(html).toMatch(/<button[^>]*disabled=""[^>]*>.*Start call/);
  });
});
