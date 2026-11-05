import { describe, expect, it } from "vitest";
import { fileFormatOf } from "./FileCard";

describe("fileFormatOf", () => {
  it("reads the format from a name or a path, whatever its case", () => {
    expect(fileFormatOf("Coron et al. (2017) - The suite.pdf")).toBe("pdf");
    expect(fileFormatOf("/Users/me/Documents/MonoBOT/Research/Paper.PDF")).toBe("pdf");
    expect(fileFormatOf("C:\\Users\\me\\Documents\\notes.md")).toBe("md");
    expect(fileFormatOf("data.csv?download=1")).toBe("csv");
  });

  it("maps close relatives and falls back to plain text", () => {
    expect(fileFormatOf("report.docx")).toBe("doc");
    expect(fileFormatOf("app.ts")).toBe("tsx");
    expect(fileFormatOf("clip.mov")).toBe("video");
    expect(fileFormatOf("archive.tgz")).toBe("gz");
    expect(fileFormatOf("README")).toBe("txt");
    expect(fileFormatOf("weird.unknownext")).toBe("txt");
  });
});
