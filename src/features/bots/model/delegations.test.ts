// @vitest-environment happy-dom
import { beforeEach, expect, it } from "vitest";
import {
  delegationsSnapshot,
  latestByAgent,
  recordDelegation,
  resetDelegationsForTest,
} from "./delegations";

beforeEach(() => {
  localStorage.clear();
  resetDelegationsForTest();
});

it("keeps the newest request to each bot, from one lead only", () => {
  recordDelegation({ from: "lead", to: "finance", message: "Old", at: 1 });
  recordDelegation({ from: "lead", to: "research", message: "Find", at: 2 });
  recordDelegation({ from: "lead", to: "finance", message: "New", at: 3 });
  recordDelegation({ from: "other", to: "finance", message: "Not lead", at: 4 });
  const rows = latestByAgent(delegationsSnapshot(), "lead");
  expect(rows.map((row) => [row.to, row.message])).toEqual([
    ["finance", "New"],
    ["research", "Find"],
  ]);
});

it("survives a reload", () => {
  recordDelegation({ from: "lead", to: "finance", message: "Budget", at: 5 });
  resetDelegationsForTest();
  expect(delegationsSnapshot()[0]?.message).toBe("Budget");
});
