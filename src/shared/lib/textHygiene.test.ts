import { describe, expect, it } from "vitest";
import { cleanText, hasHiddenMarks, stripInvisible } from "./textHygiene";

describe("text hygiene", () => {
  it("drops zero-width marks, soft hyphens, direction overrides and a BOM", () => {
    expect(
      stripInvisible("\uFEFFLa po\u00ADza de\u200B paso\u2060 \u202Etxet\u202C"),
    ).toBe("La poza de paso txet");
  });

  it("drops hidden tag characters but keeps a subdivision flag", () => {
    const hidden = [..."secret"]
      .map((char) => String.fromCodePoint(0xe0000 + char.charCodeAt(0)))
      .join("");
    expect(stripInvisible(`ok${hidden}`)).toBe("ok");
    const scotland = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}";
    expect(stripInvisible(`flag ${scotland}`)).toBe(`flag ${scotland}`);
  });

  it("keeps the joiners an emoji is made of", () => {
    const family = "👨\u200D👩\u200D👧";
    const heart = "❤\uFE0F\u200D🔥";
    expect(stripInvisible(`${family} ${heart}`)).toBe(`${family} ${heart}`);
    expect(stripInvisible("in\u200Dside")).toBe("inside");
  });

  it("keeps a non-joiner between letters that need it, not in Latin text", () => {
    expect(stripInvisible("می\u200Cخواهم")).toBe("می\u200Cخواهم");
    expect(stripInvisible("wo\u200Crd")).toBe("word");
  });

  it("makes spaces plain and trims lines and runs of blank lines", () => {
    expect(
      cleanText("Título\u00A01  \r\n\r\n\r\n\r\nUno\u202Fdos\u2028tres\n\n"),
    ).toBe("Título 1\n\nUno dos\ntres");
  });

  it("leaves a code block's spacing alone", () => {
    const code = "```py\nx  =\u00A01   \n\n\n\ny = 2\n```";
    expect(cleanText(`Antes\n\n${code}\nDespués`)).toBe(
      `Antes\n\n${code}\nDespués`,
    );
  });

  it("says whether a text has anything to clean", () => {
    expect(hasHiddenMarks("plain text")).toBe(false);
    expect(hasHiddenMarks("plain\u200B text")).toBe(true);
  });
});
