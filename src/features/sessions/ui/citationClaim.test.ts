// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import { claimRange } from "./citationClaim";

function build(html: string) {
  const host = document.createElement("div");
  host.innerHTML = html;
  document.body.append(host);
  return host;
}

const chip = (n: number) => `<a data-citation="">chip${n}</a>`;

describe("claimRange", () => {
  afterEach(() => {
    document.body.replaceChildren();
  });

  it("covers the sentence before the chip, not the ones earlier", () => {
    const host = build(
      `<p>First point. Second point is the claim ${chip(1)}. Third.</p>`,
    );
    const range = claimRange(host.querySelector("a")!);
    expect(range?.toString()).toBe("Second point is the claim");
  });

  it("stops at the previous chip inside one sentence", () => {
    const host = build(
      `<p>Fast ${chip(1)}, and it also scales ${chip(2)}.</p>`,
    );
    const [first, second] = [...host.querySelectorAll("a")];
    expect(claimRange(first)?.toString()).toBe("Fast");
    expect(claimRange(second)?.toString()).toBe("and it also scales");
  });

  it("gives chips that touch the same claim", () => {
    const host = build(`<p>One claim ${chip(1)}${chip(2)}.</p>`);
    const [first, second] = [...host.querySelectorAll("a")];
    expect(claimRange(first)?.toString()).toBe("One claim");
    expect(claimRange(second)?.toString()).toBe("One claim");
  });

  it("reads through inline formatting and takes a sentence closed before the chip", () => {
    const host = build(
      `<p>Intro. Use <strong>transform</strong> only. ${chip(1)}</p>`,
    );
    expect(claimRange(host.querySelector("a")!)?.toString()).toBe(
      "Use transform only",
    );
  });

  it("finds nothing to light up when no text comes before the chip", () => {
    const host = build(`<p>${chip(1)} then text.</p>`);
    expect(claimRange(host.querySelector("a")!)).toBeNull();
  });
});
