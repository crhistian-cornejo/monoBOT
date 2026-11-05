import { describe, expect, it, vi } from "vitest";
import {
  candidatePaths,
  collectTexts,
  locateReferencedFile,
  mentionedPaths,
} from "./referencedFile";

const NAME = "LOB_SENACE_Obs28-55-57_1064_26.xlsx";
const FULL =
  "P:/1064_26 Nexa - Act. IF Dep. de Desmonte y Contradique/03 Entregables/001/Obs 28-55-57 (Nov-2026)/" +
  NAME;

describe("mentionedPaths", () => {
  it("finds a full path with spaces and parentheses in a Markdown link", () => {
    const text = `Listo:\n\n[${NAME}](<${FULL}>) tiene la matriz.`;
    expect(mentionedPaths([text], NAME)).toContain(FULL);
  });

  it("finds Windows paths, file URLs and paths after a sentence", () => {
    const texts = [
      `Lo guardé en C:\\Users\\cc\\obra\\${NAME}, revísalo.`,
      `Abrí file:///D:/datos/${NAME}`,
      `ver notas/varias en /home/cc/obra/${NAME}`,
    ];
    const found = mentionedPaths(texts, NAME);
    expect(found).toContain(`C:\\Users\\cc\\obra\\${NAME}`);
    expect(found).toContain(`D:/datos/${NAME}`);
    // The match runs back to the slash in "notas/varias"; the path itself
    // is among the candidates.
    expect(found).toContain(`/home/cc/obra/${NAME}`);
    // The most recent mention comes first.
    expect(found[0]).toContain("/home/cc/obra/");
  });

  it("ignores other files and longer names", () => {
    expect(
      mentionedPaths([`C:/x/${NAME}.bak and C:/x/other.xlsx`], NAME),
    ).toEqual([]);
  });
});

describe("candidatePaths and locateReferencedFile", () => {
  it("tries each folder, then mentions, once each", () => {
    expect(
      candidatePaths(NAME, ["C:/Users/cc", "C:/Users/cc", undefined], [FULL]),
    ).toEqual([`C:/Users/cc/${NAME}`, FULL]);
  });

  it("returns the first candidate on disk", async () => {
    const stat = vi.fn(async (paths: string[]) =>
      paths.map((path) => ({ path, mtimeMs: path === FULL ? 1 : null })),
    );
    const texts = collectTexts([
      { kind: "text", text: `Aquí: [${NAME}](<${FULL}>)` },
    ]);
    await expect(
      locateReferencedFile(NAME, ["C:/Users/cc"], texts, stat),
    ).resolves.toBe(FULL);
    await expect(
      locateReferencedFile("otro.xlsx", ["C:/Users/cc"], texts, stat),
    ).resolves.toBeUndefined();
  });
});
