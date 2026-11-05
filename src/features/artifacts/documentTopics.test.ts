// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  documentTopicsSnapshot,
  parseTopic,
  setDocumentTopic,
  topicPrompt,
} from "./documentTopics";

describe("document topics", () => {
  it("shows the model the request, the reply, the documents and the topics so far", () => {
    const prompt = topicPrompt(
      {
        turnId: "u",
        request: "puedes buscar un paper sobre pozas de paso",
        reply: "Listo, guardé el reporte.",
        documents: ["Clark (2009): Treatment of runoff", "Informe.docx"],
      },
      ["Automatización de HEC-HMS"],
    );
    expect(prompt).toContain("- Automatización de HEC-HMS");
    expect(prompt).toContain("User's request: puedes buscar un paper sobre pozas de paso");
    expect(prompt).toContain("Documents made: Clark (2009): Treatment of runoff; Informe.docx");
    expect(prompt).toContain("language of the user's request");
  });

  it("reads the topic from a model's answer", () => {
    expect(parseTopic('"Diseño de pozas de sedimentación."\n', [])).toBe(
      "Diseño de pozas de sedimentación",
    );
    expect(parseTopic("Tema: Automatización de HEC-HMS", [])).toBe(
      "Automatización de HEC-HMS",
    );
    expect(parseTopic("automatización de hec-hms", ["Automatización de HEC-HMS"])).toBe(
      "Automatización de HEC-HMS",
    );
    expect(parseTopic("   \n", [])).toBeNull();
    expect(parseTopic("x".repeat(80), [])).toBe(`${"x".repeat(59)}…`);
  });

  it("keeps each turn's topic", () => {
    setDocumentTopic("turn-1", "Pozas");
    expect(documentTopicsSnapshot()["turn-1"]).toBe("Pozas");
  });
});
