// @vitest-environment happy-dom
import JSZip from "jszip";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DocumentView } from "./DocumentView";

const actions = vi.hoisted(() => ({
  readBinaryFile: vi.fn(async (): Promise<Uint8Array> => new Uint8Array()),
  renderAsync: vi.fn(async (_data: unknown, host: HTMLElement) => {
    host.innerHTML =
      '<div class="docx-wrapper"><section class="docx" style="width:800px">Uno</section><section class="docx">Dos</section></div>';
  }),
}));

vi.mock("../../../platform/tauri/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../platform/tauri/fs")>()),
  openPathWithDefaultApp: vi.fn(async () => {}),
  readBinaryFile: actions.readBinaryFile,
  revealPath: vi.fn(async () => {}),
}));
vi.mock("../model/fileWatch", () => ({ watchFile: () => () => {} }));
vi.mock("docx-preview", () => ({ renderAsync: actions.renderAsync }));
// No canvas under happy-dom: a PDF is three pages of a fixed size.
vi.mock("../../papers/model/pdfText", () => ({
  loadPdfjs: async () => ({
    getDocument: () => ({
      promise: Promise.resolve({
        numPages: 3,
        getPage: async () => ({
          getViewport: () => ({ width: 100, height: 141 }),
        }),
        destroy: async () => {},
      }),
    }),
  }),
}));
vi.mock("../../papers/ui/PaperPage", () => ({
  PaperPage: ({ number }: { number: number }) =>
    createElement("div", { "data-paper-page": number }),
}));

const NS =
  'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';

async function deck(): Promise<Uint8Array> {
  const zip = new JSZip();
  zip.file(
    "ppt/presentation.xml",
    `<p:presentation ${NS}><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>`,
  );
  zip.file(
    "ppt/_rels/presentation.xml.rels",
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide1.xml"/></Relationships>',
  );
  zip.file(
    "ppt/slides/slide1.xml",
    `<p:sld ${NS}><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="B"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="9144000" cy="1000000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="2400"/><a:t>Avance semanal</a:t></a:r></a:p></p:txBody></p:sp></p:spTree></p:cSld></p:sld>`,
  );
  return zip.generateAsync({ type: "uint8array" });
}

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(424);
  vi.clearAllMocks();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

async function show(path: string) {
  await act(async () => {
    root.render(createElement(DocumentView, { path, cwd: "D:/Proyectos" }));
  });
  // Reading, parsing and rendering each take a turn.
  for (let turn = 0; turn < 6; turn++)
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
}

describe("a document beside the agents' chat", () => {
  it("draws a deck's slides at the panel's width, with their text", async () => {
    actions.readBinaryFile.mockResolvedValue(await deck());
    await show("D:/Proyectos/Avance.pptx");
    const slides = container.querySelectorAll("[data-slide]");
    expect(slides).toHaveLength(1);
    expect(slides[0]!.textContent).toContain("Avance semanal");
    const surface = slides[0]!.firstElementChild as HTMLElement;
    // 424 wide, less the 12px gutter on each side; 16:9.
    expect(surface.style.width).toBe("400px");
    expect(container.textContent).toContain("1 slide");
  });

  it("renders a Word file's pages, scaled to fit, and counts them", async () => {
    actions.readBinaryFile.mockResolvedValue(new Uint8Array([1, 2, 3]));
    await show("D:/Proyectos/Memoria.docx");
    expect(actions.renderAsync).toHaveBeenCalledOnce();
    expect(actions.renderAsync.mock.calls[0]![3]).toMatchObject({
      renderAltChunks: false,
    });
    expect(container.querySelectorAll("section.docx")).toHaveLength(2);
    expect(container.textContent).toContain("2 pages");
  });

  it("lays out every page of a PDF", async () => {
    actions.readBinaryFile.mockResolvedValue(new Uint8Array([37, 80, 68, 70]));
    await show("D:/Proyectos/Informe final.pdf");
    expect(container.querySelectorAll("[data-paper-page]")).toHaveLength(3);
    expect(container.textContent).toContain("3 pages");
  });

  it("offers the system's app when a file cannot be read", async () => {
    actions.readBinaryFile.mockResolvedValue(new Uint8Array([1, 2, 3]));
    await show("D:/Proyectos/Roto.pptx");
    expect(container.textContent).toContain("Couldn’t show Roto.pptx");
  });
});
