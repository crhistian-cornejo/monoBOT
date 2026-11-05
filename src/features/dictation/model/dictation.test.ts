import { describe, expect, it } from "vitest";
import { formatModelSize, insertDictated, modelOffer } from "./dictation";

describe("insertDictated", () => {
  it("adds the spoken text at the cursor with one space around it", () => {
    expect(insertDictated("Hola", 4, 4, "qué tal")).toEqual({
      value: "Hola qué tal",
      cursor: 12,
    });
    expect(insertDictated("fix the bug", 4, 4, " now ")).toEqual({
      value: "fix now the bug",
      cursor: 7,
    });
  });

  it("replaces a selection and keeps an empty draft clean", () => {
    expect(insertDictated("say WORD here", 4, 8, "this").value).toBe(
      "say this here",
    );
    expect(insertDictated("", 0, 0, "Let's ship it.").value).toBe(
      "Let's ship it.",
    );
  });

  it("leaves the draft alone when nothing was heard", () => {
    expect(insertDictated("draft", 5, 5, "   ")).toEqual({
      value: "draft",
      cursor: 5,
    });
  });

  it("names the model size in megabytes", () => {
    expect(formatModelSize(671_122_626)).toBe("671 MB");
  });
});

describe("modelOffer", () => {
  it("offers a retry that names the size, never another app", () => {
    const offer = modelOffer(671_122_626);
    expect(offer.text).toContain("671 MB");
    expect(offer.text).not.toMatch(/Echo/);
    expect(offer.action).toBe("Download");
    expect(offer.busy).toBe("Setting up the speech model…");
  });
});
