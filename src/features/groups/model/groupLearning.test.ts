import { describe, expect, it } from "vitest";
import type { Block } from "../../sessions/model/session";
import { learningPrompt, parseLearned, pendingExchange } from "./groupLearning";
import { leadModelChoice } from "./groupVoice";
import { groupMessage, projectMemory } from "./group";

const block = (id: string, role: Block["role"], text: string, extra: Partial<Block> = {}): Block => ({
  id,
  role,
  text,
  ...extra,
});

describe("what a group learns", () => {
  const blocks: Block[] = [
    block("u1", "user", "Usemos el informe de octubre"),
    block("c1", "user", "hidden", { internal: true }),
    block("l1", "assistant", "Documents lo revisa"),
    block("p1", "assistant", "Listo: 3 m/s", {
      groupMember: {
        monoId: "d",
        name: "Documents",
        mascot: "m",
        color: "c",
        request: "r",
        at: 1,
      },
    }),
  ];

  it("reads the exchange since the mark, naming who said what", () => {
    const exchange = pendingExchange(blocks, undefined, "IVY");
    expect(exchange).toEqual({
      lastId: "p1",
      lines: [
        "User: Usemos el informe de octubre",
        "IVY: Documents lo revisa",
        "Documents: Listo: 3 m/s",
      ],
    });
    expect(pendingExchange(blocks, "p1", "IVY")).toBeUndefined();
    // Members answering after the mark wait for the user's next message.
    expect(pendingExchange(blocks, "l1", "IVY")).toBeUndefined();
    const prompt = learningPrompt("Nexa", "Informe IF", "- 2026-11-01 · Lima", exchange!);
    expect(prompt).toContain('"Nexa", which is for: Informe IF');
    expect(prompt).toContain("Documents: Listo: 3 m/s");
    expect(prompt).toContain("NONE");
  });

  it("keeps new short facts only", () => {
    const memory = "- 2026-11-01 · El cliente es Nexa";
    expect(parseLearned("NONE", memory)).toEqual([]);
    expect(
      parseLearned(
        "Facts:\n- El cliente es Nexa\n- Usar el informe de octubre\n* Usar el informe de octubre\n- none\n- " +
          "x".repeat(400),
        memory,
      ),
    ).toEqual(["Usar el informe de octubre"]);
  });

  it("hands members the project's live memory", () => {
    expect(projectMemory("# Memory\n- a\n- ~~old~~\n* b\ntext")).toBe("- a\n* b");
    const message = groupMessage("Nexa", undefined, "Revisa", undefined, "- 2026 · Usar octubre");
    expect(message).toContain("What the project remembers");
    expect(message).toContain("- 2026 · Usar octubre");
    expect(groupMessage("Nexa", undefined, "Revisa")).not.toContain("remembers");
  });
});

describe("the group's voice", () => {
  const lead = {
    harness: "claude" as const,
    model: "opus",
    modelSettings: { effort: "high" },
    runtimeMode: "auto" as const,
  };

  it("takes the Principal's model, settings and permissions", () => {
    expect(leadModelChoice({ ...lead, model: "sonnet" }, lead)).toEqual(lead);
    expect(
      leadModelChoice({ ...lead, modelSettings: { effort: "medium" } }, lead),
    ).toEqual(lead);
    expect(leadModelChoice({ ...lead }, lead)).toBeUndefined();
  });
});
