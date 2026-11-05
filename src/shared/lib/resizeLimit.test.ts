// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  clearResizeLimit,
  limitPull,
  limitStrain,
  paintResizeLimit,
} from "./resizeLimit";

describe("limitStrain", () => {
  it("stays dark inside the limits and for rounding noise", () => {
    expect(limitStrain(0)).toBe(0);
    expect(limitStrain(0.5)).toBe(0);
    expect(limitStrain(-0.4)).toBe(0);
    expect(limitStrain(Number.NaN)).toBe(0);
  });

  it("shows at once at the limit and grows toward 1 either way", () => {
    const near = limitStrain(2);
    const far = limitStrain(200);
    expect(near).toBeGreaterThan(0.3);
    expect(far).toBeGreaterThan(near);
    expect(far).toBeLessThanOrEqual(1);
    expect(limitStrain(-200)).toBe(far);
  });
});

describe("limitPull", () => {
  it("leans the way the pointer pushes, a few px at most", () => {
    expect(limitPull(0)).toBe(0);
    expect(limitPull(500)).toBeGreaterThan(0);
    expect(limitPull(-500)).toBeLessThan(0);
    expect(Math.abs(limitPull(10_000))).toBeLessThanOrEqual(4);
  });
});

describe("paintResizeLimit", () => {
  it("marks the handle and fades it back out", () => {
    const handle = document.createElement("div");
    paintResizeLimit(handle, "x", 40, 120);
    expect(handle.dataset.resizeLimit).toBe("x");
    expect(Number(handle.style.getPropertyValue("--resize-strain"))).toBe(
      Number(limitStrain(40).toFixed(3)),
    );
    expect(handle.style.getPropertyValue("--resize-at")).toBe("120px");

    clearResizeLimit(handle);
    expect(handle.dataset.resizeLimit).toBe("x");
    expect(handle.style.getPropertyValue("--resize-strain")).toBe("0");
  });
});
