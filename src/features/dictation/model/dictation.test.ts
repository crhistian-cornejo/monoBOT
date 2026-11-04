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
  it("offers the download when no other app has the model", () => {
    const offer = modelOffer(671_122_626, null);
    expect(offer.text).toContain("Download the speech model (671 MB");
    expect(offer.action).toBe("Download");
    expect(offer.busy).toBe("Downloading the speech model…");
  });

  it("offers Echo's copy, with nothing to download", () => {
    const offer = modelOffer(671_122_626, "Echo");
    expect(offer.text).toContain("Echo already has the speech model");
    expect(offer.text).not.toContain("MB");
    expect(offer.action).toBe("Use it");
    expect(offer.busy).toBe("Setting up the speech model from Echo…");
  });
});
