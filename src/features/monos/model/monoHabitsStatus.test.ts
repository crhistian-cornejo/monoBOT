// @vitest-environment happy-dom
import { expect, it } from "vitest";
import {
  afterRun,
  failedInARow,
  HABIT_FAILURES_TO_PAUSE,
  habitRunPrompt,
  parseHabits,
  reportStatus,
  serializeHabits,
  type Habit,
} from "./monoHabits";

const habit = (patch: Partial<Habit> = {}): Habit => ({
  id: "h1",
  name: "Bank review",
  instructions: "Review the bank's emails.",
  schedule: { scheduleKind: "daily", time: "09:00", minute: 0, dayOfWeek: 1 },
  enabled: true,
  createdAt: 1,
  nextRunAt: 2,
  ...patch,
});

it("reads a report's status tag and leaves plain reports as progress", () => {
  expect(reportStatus("[needs_attention] Card charged twice.")).toEqual({
    status: "needs_attention",
    text: "Card charged twice.",
  });
  expect(reportStatus("[BLOCKED] Gmail is not connected")).toEqual({
    status: "blocked",
    text: "Gmail is not connected",
  });
  expect(reportStatus("Spent S/ 45 today.")).toEqual({
    status: "progress",
    text: "Spent S/ 45 today.",
  });
});

it("pauses a habit after failing too many times in a row", () => {
  let current = habit();
  for (let run = 1; run < HABIT_FAILURES_TO_PAUSE; run++) {
    current = afterRun(current, 10 + run, { kind: "failed", error: "down" });
    expect(current.enabled).toBe(true);
  }
  current = afterRun(current, 100, { kind: "failed", error: "down" });
  expect(failedInARow(current)).toBe(HABIT_FAILURES_TO_PAUSE);
  expect(current.enabled).toBe(false);
});

it("starts counting failures again after a good run", () => {
  let current = habit();
  current = afterRun(current, 1, { kind: "failed", error: "x" });
  current = afterRun(current, 2, { kind: "quiet" });
  current = afterRun(current, 3, { kind: "failed", error: "x" });
  expect(failedInARow(current)).toBe(1);
});

it("keeps a run's status and gives the next run the last report", () => {
  const ran = afterRun(habit(), 5, {
    kind: "posted",
    report: "Spent S/ 320 on delivery, 80% of the cap.",
    status: "needs_attention",
  });
  const stored = parseHabits(serializeHabits([ran]))[0]!;
  expect(stored.runs?.[0]?.status).toBe("needs_attention");
  const prompt = habitRunPrompt(
    { name: "Finance", mascot: "invader", color: "#000", projects: [] },
    { soul: "", memory: "", topics: [] } as never,
    stored,
    10,
  );
  expect(prompt).toContain("Last time it told the user: Spent S/ 320");
  expect(prompt).toContain("[needs_attention]");
});
