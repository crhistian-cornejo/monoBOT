// @vitest-environment happy-dom
import ExcelJS from "exceljs";
import { act, createElement, StrictMode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SpreadsheetView } from "./SpreadsheetView";

const actions = vi.hoisted(() => ({
  openPathWithDefaultApp: vi.fn(async () => {}),
  readBinaryFile: vi.fn(async (): Promise<Uint8Array> => new Uint8Array()),
}));

vi.mock("../../../platform/tauri/fs", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../platform/tauri/fs")>()),
  openPathWithDefaultApp: actions.openPathWithDefaultApp,
  readBinaryFile: actions.readBinaryFile,
  revealPath: vi.fn(async () => {}),
}));

vi.mock("../model/fileWatch", () => ({ watchFile: () => () => {} }));

// No workers under happy-dom: read on this thread.
vi.mock("../model/spreadsheetWorker", async () => {
  const { parseWorkbook } = await import("../model/spreadsheet");
  return {
    readSpreadsheet: (bytes: Uint8Array, path: string) =>
      parseWorkbook(bytes, path, "en-US"),
  };
});

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
  // A viewport for the grid to draw into.
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(600);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(400);
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

async function open(path: string, bytes: Uint8Array) {
  actions.readBinaryFile.mockResolvedValue(bytes);
  await act(async () => {
    root.render(createElement(SpreadsheetView, { path, cwd: "/obra" }));
  });
  // The read and the parse (an unzip, for a workbook) settle over a while.
  for (let turn = 0; turn < 100; turn++) {
    if (!container.textContent?.includes("Opening")) break;
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
    });
  }
}

const formulaBar = () =>
  container.querySelector("[title='Open in default app']")!.parentElement!
    .parentElement!;

describe("SpreadsheetView", () => {
  it("draws the cells and follows the picked cell in the formula bar", async () => {
    await open(
      "/obra/obs.csv",
      new TextEncoder().encode("Item,Volumen\nCantera,340000\n"),
    );
    const grid = container.querySelector<HTMLElement>("[role='grid']")!;
    expect(grid.textContent).toContain("Cantera");
    expect(grid.textContent).toContain("340000");
    expect(formulaBar().textContent).toContain("A1");
    expect(formulaBar().textContent).toContain("Item");

    act(() => {
      grid.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }),
      );
    });
    act(() => {
      grid.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true }),
      );
    });
    expect(formulaBar().textContent).toContain("B2");
    expect(formulaBar().textContent).toContain("340000");
  });

  it("switches between a workbook's sheets", async () => {
    const workbook = new ExcelJS.Workbook();
    workbook.addWorksheet("LOB").getCell("A1").value = "Observación 28";
    workbook.addWorksheet("RFI 002").getCell("A1").value = "Plantilla RFI";
    const bytes = new Uint8Array(await workbook.xlsx.writeBuffer());
    await open("/obra/LOB.xlsx", bytes);

    const tabs = [...container.querySelectorAll<HTMLElement>("[role='tab']")];
    expect(tabs.map((tab) => tab.textContent)).toEqual(["LOB", "RFI 002"]);
    expect(container.textContent).toContain("Observación 28");

    act(() => tabs[1].click());
    expect(container.textContent).toContain("Plantilla RFI");
    expect(container.textContent).not.toContain("Observación 28");
  });

  it("shows formulas in the cells on the fx button, and back", async () => {
    const workbook = new ExcelJS.Workbook();
    const sheet = workbook.addWorksheet("Cálculos");
    sheet.getCell("A1").value = 420;
    sheet.getCell("B1").value = {
      formula: "A1/1000*2",
      result: 0.84,
    } as ExcelJS.CellValue;
    const bytes = new Uint8Array(await workbook.xlsx.writeBuffer());
    await open("/obra/calc.xlsx", bytes);

    const grid = container.querySelector<HTMLElement>("[role='grid']")!;
    expect(grid.textContent).toContain("0.84");
    expect(grid.textContent).not.toContain("=A1/1000*2");

    const fx = container.querySelector<HTMLButtonElement>(
      "button[aria-pressed]",
    )!;
    act(() => fx.click());
    expect(fx.getAttribute("aria-pressed")).toBe("true");
    expect(grid.textContent).toContain("=A1/1000*2");
    // Plain values still show as they are.
    expect(grid.textContent).toContain("420");

    act(() => fx.click());
    expect(grid.textContent).toContain("0.84");
  });

  it("draws a workbook's pictures over its cells", async () => {
    const png = Uint8Array.from(
      atob(
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
      ),
      (char) => char.charCodeAt(0),
    );
    let made = 0;
    vi.spyOn(URL, "createObjectURL").mockImplementation(
      () => `blob:figura-${++made}`,
    );
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
    const workbook = new ExcelJS.Workbook();
    const sheet = workbook.addWorksheet("Figuras");
    sheet.getCell("A1").value = "Figura 1";
    sheet.addImage(
      workbook.addImage({ buffer: png as never, extension: "png" }),
      "B2:D6",
    );
    const bytes = new Uint8Array(await workbook.xlsx.writeBuffer());
    // StrictMode mounts twice: the picture must survive the first cleanup.
    actions.readBinaryFile.mockResolvedValue(bytes);
    await act(async () => {
      root.render(
        createElement(
          StrictMode,
          null,
          createElement(SpreadsheetView, {
            path: "/obra/fig.xlsx",
            cwd: "/obra",
          }),
        ),
      );
    });
    for (let turn = 0; turn < 100; turn++) {
      if (container.querySelector("[role='grid'] img")) break;
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 10));
      });
    }
    expect(URL.revokeObjectURL).not.toHaveBeenCalledWith(
      container.querySelector("[role='grid'] img")?.getAttribute("src"),
    );

    const image =
      container.querySelector<HTMLImageElement>("[role='grid'] img")!;
    expect(image.getAttribute("src")).toMatch(/^blob:figura-/);
    expect(image.style.width).toBe("192px");
    expect(image.style.height).toBe("100px");
  });

  it("sends the old .xls format to Excel", async () => {
    await open("/obra/old.xls", new Uint8Array([0xd0, 0xcf, 0x11, 0xe0]));
    expect(container.textContent).toContain("old binary Excel format");
    const openButton = [...container.querySelectorAll("button")].find(
      (button) => button.textContent === "Open externally",
    )!;
    act(() => openButton.click());
    expect(actions.openPathWithDefaultApp).toHaveBeenCalledWith(
      "/obra/old.xls",
    );
  });
});
