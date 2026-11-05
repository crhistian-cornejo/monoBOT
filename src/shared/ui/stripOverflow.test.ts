import { describe, expect, it } from "vitest";
import { revealScrollLeft, stripOverflow } from "./stripOverflow";

describe("stripOverflow", () => {
  it("has no overflow when the strip fits", () => {
    expect(stripOverflow(0, 400, 400)).toEqual({ left: false, right: false });
  });

  it("has only right overflow at the start", () => {
    expect(stripOverflow(0, 400, 800)).toEqual({ left: false, right: true });
  });

  it("has both in the middle", () => {
    expect(stripOverflow(200, 400, 800)).toEqual({ left: true, right: true });
  });

  it("has only left overflow at the end", () => {
    expect(stripOverflow(400, 400, 800)).toEqual({ left: true, right: false });
  });
});

describe("revealScrollLeft", () => {
  it("returns to the start for the first item", () => {
    expect(revealScrollLeft(8, 50, 20, 300, 24)).toBe(0);
  });

  it("leaves a visible item where it is", () => {
    expect(revealScrollLeft(100, 50, 20, 300, 24)).toBe(20);
  });

  it("scrolls left to an item cut off on the left", () => {
    expect(revealScrollLeft(100, 50, 120, 300, 24)).toBe(76);
  });

  it("scrolls right to an item cut off on the right", () => {
    expect(revealScrollLeft(300, 60, 0, 300, 24)).toBe(84);
  });
});
