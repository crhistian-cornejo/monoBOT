import ExcelJS from "exceljs";
import { describe, expect, it } from "vitest";
import { parseWorkbook, themeColors, type Sheet } from "./spreadsheet";
import {
  parseChart,
  parseXml,
  rangeReader,
  resolvePart,
} from "./spreadsheetDrawings";

const PNG = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  ),
  (char) => char.charCodeAt(0),
);

const C = 'xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"';
const A = 'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"';
const theme = themeColors(undefined);
const noRanges = () => ({ texts: [], numbers: [] });

describe("parseXml", () => {
  it("reads local names, attributes, entities and text", () => {
    const root = parseXml(
      '<?xml version="1.0"?><c:a r:id="rId1" val=\'x &amp; y\'><c:b>1 &lt; 2</c:b><c:c/></c:a>',
    );
    const a = root.children[0];
    expect(a.name).toBe("a");
    expect(a.attrs["r:id"]).toBe("rId1");
    expect(a.attrs.id).toBe("rId1");
    expect(a.attrs.val).toBe("x & y");
    expect(a.children.map((node) => node.name)).toEqual(["b", "c"]);
    expect(a.children[0].text).toBe("1 < 2");
  });

  it("resolves package parts", () => {
    expect(
      resolvePart("xl/worksheets/sheet1.xml", "../drawings/drawing1.xml"),
    ).toBe("xl/drawings/drawing1.xml");
    expect(resolvePart("xl/workbook.xml", "/xl/worksheets/sheet2.xml")).toBe(
      "xl/worksheets/sheet2.xml",
    );
  });
});

describe("parseChart", () => {
  it("reads a cached column chart: title, series, colours, axes", () => {
    const xml = `<c:chartSpace ${C} ${A}><c:chart>
      <c:title><c:tx><c:rich><a:p><a:r><a:t>Balance</a:t></a:r></a:p></c:rich></c:tx></c:title>
      <c:plotArea><c:barChart><c:barDir val="col"/><c:grouping val="clustered"/>
        <c:ser><c:tx><c:strRef><c:f>S!$B$1</c:f><c:strCache><c:pt idx="0"><c:v>Oferta</c:v></c:pt></c:strCache></c:strRef></c:tx>
          <c:spPr><a:solidFill><a:srgbClr val="ED7D31"/></a:solidFill></c:spPr>
          <c:cat><c:strRef><c:f>S!$A$2:$A$3</c:f><c:strCache><c:ptCount val="2"/><c:pt idx="0"><c:v>a</c:v></c:pt><c:pt idx="1"><c:v>b</c:v></c:pt></c:strCache></c:strRef></c:cat>
          <c:val><c:numRef><c:f>S!$B$2:$B$3</c:f><c:numCache><c:ptCount val="2"/><c:pt idx="0"><c:v>340000</c:v></c:pt><c:pt idx="1"><c:v>1066</c:v></c:pt></c:numCache></c:numRef></c:val>
        </c:ser>
        <c:ser><c:tx><c:v>Demanda</c:v></c:tx>
          <c:val><c:numRef><c:numCache><c:ptCount val="2"/><c:pt idx="1"><c:v>500</c:v></c:pt></c:numCache></c:numRef></c:val>
        </c:ser>
      </c:barChart>
      <c:catAx><c:axPos val="b"/><c:title><c:tx><c:rich><a:p><a:r><a:t>Ítem</a:t></a:r></a:p></c:rich></c:tx></c:title></c:catAx>
      <c:valAx><c:axPos val="l"/><c:title><c:tx><c:rich><a:p><a:r><a:t>m³</a:t></a:r></a:p></c:rich></c:tx></c:title></c:valAx>
      </c:plotArea><c:legend/></c:chart></c:chartSpace>`;
    const chart = parseChart(xml, theme, noRanges);
    expect(chart).toMatchObject({
      title: "Balance",
      categories: ["a", "b"],
      legend: true,
      xTitle: "Ítem",
      yTitle: "m³",
    });
    expect(chart.series).toEqual([
      {
        kind: "column",
        name: "Oferta",
        values: [340000, 1066],
        color: "#ed7d31",
      },
      // A missing point stays empty; the colour falls back to the theme.
      {
        kind: "column",
        name: "Demanda",
        values: [null, 500],
        color: "#ed7d31",
      },
    ]);
  });

  it("reads the cells when a chart was saved without its cache", () => {
    // openpyxl writes references only; Excel fills the cache when it saves.
    const sheet: Sheet = {
      name: "Datos 1",
      rowCount: 3,
      colCount: 2,
      colWidths: [64, 64],
      rowHeights: [20, 20, 20],
      frozenRows: 0,
      frozenCols: 0,
      gridlines: true,
      truncated: false,
      cells: [
        { row: 0, col: 1, text: "Caudal", input: "Caudal" },
        { row: 1, col: 0, text: "TR100", input: "TR100" },
        { row: 1, col: 1, text: "99", input: "99", numeric: true, value: 99 },
        { row: 2, col: 0, text: "TR200", input: "TR200" },
        {
          row: 2,
          col: 1,
          text: "120.5",
          input: "=B2*1.2",
          numeric: true,
          value: 120.5,
        },
      ],
    };
    const xml = `<c:chartSpace ${C}><c:chart><c:plotArea><c:lineChart><c:grouping val="standard"/>
      <c:ser><c:tx><c:strRef><c:f>'Datos 1'!$B$1</c:f></c:strRef></c:tx>
      <c:cat><c:strRef><c:f>'Datos 1'!$A$2:$A$3</c:f></c:strRef></c:cat>
      <c:val><c:numRef><c:f>'Datos 1'!$B$2:$B$3</c:f></c:numRef></c:val></c:ser>
      </c:lineChart></c:plotArea></c:chart></c:chartSpace>`;
    const chart = parseChart(
      xml,
      theme,
      rangeReader(new Map([["Datos 1", sheet]])),
    );
    expect(chart.categories).toEqual(["TR100", "TR200"]);
    expect(chart.series[0]).toMatchObject({
      kind: "line",
      name: "Caudal",
      values: [99, 120.5],
      color: theme[4],
    });
  });

  it("gives a pie its point colours and names charts it does not draw", () => {
    const pie = parseChart(
      `<c:chartSpace ${C} ${A}><c:chart><c:plotArea><c:pieChart><c:ser>
        <c:dPt><c:idx val="1"/><c:spPr><a:solidFill><a:schemeClr val="accent6"/></a:solidFill></c:spPr></c:dPt>
        <c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt><c:pt idx="2"><c:v>3</c:v></c:pt></c:numLit></c:val>
      </c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>`,
      theme,
      noRanges,
    );
    expect(pie.series[0].pointColors).toEqual([theme[4], theme[9], theme[6]]);
    expect(pie.categories).toEqual(["1", "2", "3"]);

    const radar = parseChart(
      `<c:chartSpace ${C}><c:chart><c:plotArea><c:radarChart/></c:plotArea></c:chart></c:chartSpace>`,
      theme,
      noRanges,
    );
    expect(radar.unsupported).toBe("radar");
  });
});

describe("parseWorkbook drawings", () => {
  it("places a picture by the cells it is anchored to", async () => {
    const workbook = new ExcelJS.Workbook();
    const sheet = workbook.addWorksheet("Figuras");
    sheet.getColumn(1).width = 20;
    const image = workbook.addImage({ buffer: PNG as never, extension: "png" });
    sheet.addImage(image, "B3:D8");
    workbook.addWorksheet("Vacía").getCell("A1").value = 1;
    const bytes = new Uint8Array(await workbook.xlsx.writeBuffer());

    const { sheets } = await parseWorkbook(bytes, "/obra/fig.xlsx");
    const [figuras, vacia] = sheets;
    expect(vacia.drawings).toBeUndefined();
    expect(figuras.drawings).toHaveLength(1);
    const picture = figuras.drawings![0];
    expect(picture.kind).toBe("image");
    if (picture.kind !== "image") return;
    expect(picture.extension).toBe("png");
    expect([...picture.bytes]).toEqual([...PNG]);
    // Column A is 20 characters wide (145 px); rows are 20 px.
    expect(picture.box.x).toBe(145);
    expect(picture.box.y).toBe(40);
    expect(picture.box.width).toBeCloseTo(64 * 3, 0);
    expect(picture.box.height).toBeCloseTo(20 * 6, 0);
  });

  it("keeps a chart's numbers on its cells", async () => {
    const workbook = new ExcelJS.Workbook();
    const sheet = workbook.addWorksheet("Cálculos");
    sheet.getCell("B2").value = {
      formula: "1+1",
      result: 2,
    } as ExcelJS.CellValue;
    const bytes = new Uint8Array(await workbook.xlsx.writeBuffer());
    const { sheets } = await parseWorkbook(bytes, "/obra/c.xlsx");
    expect(sheets[0].cells[0]).toMatchObject({
      input: "=1+1",
      text: "2",
      value: 2,
    });
  });
});
