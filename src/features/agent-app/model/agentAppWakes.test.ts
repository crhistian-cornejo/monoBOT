import { describe, expect, it } from "vitest";
import { wakeTime } from "./agentApp";

describe("wakeTime", () => {
  const now = Date.UTC(2026, 10, 4, 12);

  it("takes minutes from now or a date and time", () => {
    expect(wakeTime({ in: 30 }, now)).toBe(now + 30 * 60_000);
    expect(wakeTime({ at: "2026-11-05T09:30:00Z" }, now)).toBe(
      Date.UTC(2026, 10, 5, 9, 30),
    );
  });

  it("keeps wakes between five minutes and seven days ahead", () => {
    expect(() => wakeTime({ in: 2 }, now)).toThrow("5 minutes");
    expect(() => wakeTime({ in: 60 * 24 * 8 }, now)).toThrow("7 days");
    expect(() => wakeTime({}, now)).toThrow('Give "in"');
    expect(() => wakeTime({ at: "soon" }, now)).toThrow("at must be");
  });
});
