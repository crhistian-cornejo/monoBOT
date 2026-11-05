// @vitest-environment happy-dom
import { expect, it } from "vitest";
import { agentsNeedingYou, type Mono } from "./mono";

const agent = (id: string, name: string, sessionId?: string): Mono => ({
  id,
  name,
  mascot: "cat",
  color: "#fff",
  projects: [],
  kind: "bot",
  botRole: "assistant",
  ...(sessionId ? { sessionId } : {}),
});

it("lists every agent waiting on the user, with what it waits for", () => {
  const sessions = new Map([
    [
      "s-ivy",
      {
        busy: true,
        blocks: [
          {
            id: "1",
            role: "assistant" as const,
            text: "",
            tool: { title: "Read /tmp/report.pdf" },
            approval: { requestId: 1 },
          },
        ],
      },
    ],
    ["s-ledger", { busy: true, blocks: [] }],
    ["s-scout", { busy: false, blocks: [], usageLimit: { resetsAt: 1000 } }],
  ]);
  expect(
    agentsNeedingYou(
      [
        agent("ivy", "IVY", "s-ivy"),
        agent("ledger", "Ledger", "s-ledger"),
        agent("scout", "Scout", "s-scout"),
        // Never opened: nothing to wait on.
        agent("new", "New"),
      ],
      (id) => sessions.get(id),
    ),
  ).toEqual([
    { monoId: "ivy", name: "IVY", activity: "Approve Read /tmp/report.pdf" },
    { monoId: "scout", name: "Scout", activity: "Usage limit reached" },
  ]);
});
