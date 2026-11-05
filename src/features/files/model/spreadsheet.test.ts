import ExcelJS from "exceljs";
import { describe, expect, it } from "vitest";
import {
  applyTint,
  cellAddress,
  columnName,
  columnPixels,
  csvWorkbook,
  excelColor,
  formatDate,
  formatNumber,
  isSpreadsheetPath,
  isWorkbookPath,
  parseAddress,
  parseDelimited,
  parseWorkbook,
  readableOn,
  sniffDelimiter,
  themeColors,
} from "./spreadsheet";

describe("addresses", () => {
  it("names columns as Excel does", () => {
    expect([0, 25, 26, 51, 52, 701, 702].map(columnName)).toEqual([
      "A",
      "Z",
      "AA",
      "AZ",
      "BA",
      "ZZ",
      "AAA",
    ]);
    expect(cellAddress(4, 1)).toBe("B5");
    expect(parseAddress("$AB$12")).toEqual({ row: 11, col: 27 });
    expect(parseAddress("12")).toBeNull();
  });

  it("routes spreadsheet files to the viewer", () => {
    expect(isSpreadsheetPath("C:\\obra\\LOB_SENACE.xlsx")).toBe(true);
    expect(isSpreadsheetPath("/tmp/data.CSV")).toBe(true);
    expect(isSpreadsheetPath("/tmp/old.xls")).toBe(true);
    expect(isSpreadsheetPath("/tmp/notes.md")).toBe(false);
    // CSV stays editable text in the editor.
    expect(isWorkbookPath("/tmp/data.csv")).toBe(false);
    expect(isWorkbookPath("/tmp/LOB.XLSX")).toBe(true);
  });
});

describe("colours", () => {
  it("reads a theme with light colours first", () => {
    const xml =
      '<a:dk1><a:sysClr val="windowText" lastClr="111111"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FEFEFE"/></a:lt1>' +
      '<a:dk2><a:srgbClr val="222222"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="FF0000"/></a:accent1>';
    const theme = themeColors(xml);
    expect(theme.slice(0, 5)).toEqual([
      "#fefefe",
      "#111111",
      "#eeeeee",
      "#222222",
      "#ff0000",
    ]);
    // Missing entries keep Office's.
    expect(theme[5]).toBe("#ed7d31");
  });

  it("tints toward white and black", () => {
    expect(applyTint("#000000", 0.5)).toBe("#808080");
    expect(applyTint("#ffffff", -0.5)).toBe("#808080");
    expect(excelColor({ theme: 5, tint: 0.4 }, themeColors(undefined))).toBe(
      applyTint("#ed7d31", 0.4),
    );
    expect(excelColor({ argb: "FFED7D31" }, [])).toBe("#ed7d31");
  });

  it("picks text that reads on a fill", () => {
    expect(readableOn("#ffff00")).toBe("#000000");
    expect(readableOn("#1f3864")).toBe("#ffffff");
  });
});

describe("numbers", () => {
  const f = (value: number, format?: string) =>
    formatNumber(value, format, "en-US");

  it("prints General without padding", () => {
    expect(f(3)).toBe("3");
    expect(f(0.1 + 0.2)).toBe("0.3");
    expect(f(1234567.5, "General")).toBe("1234567.5");
  });

  it("keeps decimals, thousands, percent and units", () => {
    expect(f(1066, "#,##0")).toBe("1,066");
    expect(f(340000.456, "#,##0.00")).toBe("340,000.46");
    expect(f(0.256, "0.0%")).toBe("25.6%");
    expect(f(74, '0 "m"')).toBe("74 m");
    expect(f(12.5, "[$S/-280A] #,##0.00")).toBe("S/ 12.50");
    expect(f(1.5, "0.##")).toBe("1.5");
  });

  it("uses the negative and zero sections", () => {
    expect(f(-1500, "#,##0;(#,##0)")).toBe("(1,500)");
    expect(f(-2, "0;[Red]-0")).toBe("-2");
    expect(f(0, '0;-0;"-"')).toBe("-");
  });

  it("prints dates in UTC as the format says", () => {
    const date = new Date(Date.UTC(2026, 10, 5, 14, 7, 9));
    expect(formatDate(date, "dd/mm/yyyy")).toBe("05/11/2026");
    expect(formatDate(date, "yyyy-mm-dd hh:mm:ss")).toBe("2026-11-05 14:07:09");
    expect(formatDate(date, "h:mm AM/PM")).toBe("2:07 PM");
    expect(formatDate(date, "d-mmm-yy", "en-US")).toBe("5-Nov-26");
  });
});

describe("delimited text", () => {
  it("splits quoted fields with delimiters and line breaks", () => {
    expect(parseDelimited('a,"b,c","d ""e"""\r\n1,"2\n3",\n', ",")).toEqual([
      ["a", "b,c", 'd "e"'],
      ["1", "2\n3", ""],
    ]);
  });

  it("sniffs semicolons and tabs", () => {
    expect(sniffDelimiter("a;b;c\n1;2;3", "x.csv")).toBe(";");
    expect(sniffDelimiter("a,b", "x.tsv")).toBe("\t");
    expect(sniffDelimiter("single", "x.csv")).toBe(",");
  });

  it("builds a sheet from CSV, numbers leaning right", () => {
    const { sheets } = csvWorkbook(
      "\uFEFFItem,Volumen\na,340000\n",
      "/p/obs.csv",
    );
    expect(sheets[0].name).toBe("obs");
    expect(sheets[0].cells).toEqual([
      { row: 0, col: 0, text: "Item", input: "Item" },
      { row: 0, col: 1, text: "Volumen", input: "Volumen" },
      { row: 1, col: 0, text: "a", input: "a" },
      { row: 1, col: 1, text: "340000", input: "340000", numeric: true },
    ]);
    expect(sheets[0].rowCount).toBeGreaterThanOrEqual(2);
  });
});

async function workbookBytes(
  build: (workbook: ExcelJS.Workbook) => void,
): Promise<Uint8Array> {
  const workbook = new ExcelJS.Workbook();
  build(workbook);
  return new Uint8Array(await workbook.xlsx.writeBuffer());
}

describe("parseWorkbook", () => {
  it("reads values, formulas, styles, merges and layout", async () => {
    const bytes = await workbookBytes((workbook) => {
      const sheet = workbook.addWorksheet("LOB");
      sheet.getCell("A1").value = "Qué pide SENACE";
      sheet.getCell("A1").font = { bold: true, color: { argb: "FFFFFFFF" } };
      sheet.getCell("A1").fill = {
        type: "pattern",
        pattern: "solid",
        fgColor: { argb: "FFED7D31" },
      };
      sheet.getCell("A1").alignment = {
        horizontal: "center",
        vertical: "middle",
        wrapText: true,
      };
      sheet.getCell("B2").value = 1066;
      sheet.getCell("B2").numFmt = "#,##0";
      sheet.getCell("C2").value = { formula: "B2*2", result: 2132 };
      sheet.getCell("D2").value = { formula: 'A1&"x"' } as ExcelJS.CellValue;
      sheet.getCell("E2").value = { error: "#N/A" } as ExcelJS.CellValue;
      sheet.getCell("F2").value = new Date(Date.UTC(2026, 10, 5));
      sheet.getCell("F2").numFmt = "dd/mm/yyyy";
      sheet.getCell("B3").font = { color: { argb: "FFC00000" } };
      sheet.getCell("B3").value = "rojo";
      sheet.getCell("B3").border = {
        bottom: { style: "medium", color: { argb: "FF000000" } },
      };
      sheet.mergeCells("A5:A8");
      sheet.getCell("A5").value = 28;
      sheet.getColumn(2).width = 30;
      sheet.getColumn(7).hidden = true;
      sheet.getRow(5).height = 30;
      sheet.views = [
        { state: "frozen", xSplit: 1, ySplit: 4, showGridLines: false },
      ];
      workbook.addWorksheet("Obs 28 detalle").getCell("A1").value = "otra";
      workbook.addWorksheet("Oculta", { state: "hidden" });
    });
    const { sheets } = await parseWorkbook(bytes, "C:/obra/LOB.xlsx", "en-US");
    expect(sheets.map((sheet) => sheet.name)).toEqual([
      "LOB",
      "Obs 28 detalle",
    ]);
    const [lob] = sheets;
    const cell = (address: string) => {
      const at = parseAddress(address)!;
      return lob.cells.find(
        (entry) => entry.row === at.row && entry.col === at.col,
      );
    };

    expect(cell("A1")).toMatchObject({
      text: "Qué pide SENACE",
      style: {
        fill: "#ed7d31",
        bold: true,
        align: "center",
        valign: "middle",
        wrap: true,
      },
    });
    // White on a fill is Excel's automatic colour: the viewer picks it.
    expect(cell("A1")?.style?.color).toBeUndefined();
    expect(cell("B2")).toMatchObject({
      text: "1,066",
      input: "1066",
      numeric: true,
    });
    expect(cell("C2")).toMatchObject({ text: "2132", input: "=B2*2" });
    expect(cell("D2")).toMatchObject({
      text: '=A1&"x"',
      input: '=A1&"x"',
      uncalculated: true,
    });
    expect(cell("E2")).toMatchObject({ text: "#N/A", error: true });
    expect(cell("F2")).toMatchObject({
      text: "05/11/2026",
      input: "2026-11-05",
    });
    expect(cell("B3")?.style).toMatchObject({
      color: "#c00000",
      bottom: { width: 2, style: "solid" },
    });
    expect(cell("B3")?.style?.bottom?.color).toBeUndefined();
    expect(cell("A5")).toMatchObject({ text: "28", rowSpan: 4, colSpan: 1 });
    expect(lob.cells.some((entry) => entry.row === 5 && entry.col === 0)).toBe(
      false,
    );

    expect(lob.colWidths[1]).toBe(columnPixels(30));
    expect(lob.colWidths[6]).toBe(0);
    expect(lob.rowHeights[4]).toBe(40);
    expect(lob.frozenRows).toBe(4);
    expect(lob.frozenCols).toBe(1);
    expect(lob.gridlines).toBe(false);
    expect(lob.truncated).toBe(false);
  });

  it("explains files it cannot read", async () => {
    await expect(
      parseWorkbook(new Uint8Array([1, 2, 3]), "/x/old.xls"),
    ).rejects.toThrow(/old binary Excel format/);
    await expect(
      parseWorkbook(new TextEncoder().encode("not a zip"), "/x/broken.xlsx"),
    ).rejects.toThrow(/could not be read as a workbook/);
  });
});
