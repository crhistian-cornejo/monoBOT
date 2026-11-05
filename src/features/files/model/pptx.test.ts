// @vitest-environment happy-dom
import JSZip from "jszip";
import { describe, expect, it } from "vitest";
import { documentKind } from "./document";
import { readPptx, resolvePart } from "./pptx";

const NS =
  'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"';
const REL = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

const rels = (entries: [string, string, string][]) =>
  `<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">${entries
    .map(
      ([id, type, target]) =>
        `<Relationship Id="${id}" Type="${REL}/${type}" Target="${target}"/>`,
    )
    .join("")}</Relationships>`;

const PNG = Uint8Array.from(
  atob(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  ),
  (c) => c.charCodeAt(0),
);

async function deck(): Promise<Uint8Array> {
  const zip = new JSZip();
  zip.file(
    "ppt/presentation.xml",
    `<p:presentation ${NS}><p:sldIdLst><p:sldId id="257" r:id="rId2"/><p:sldId id="256" r:id="rId1"/><p:sldId id="258" r:id="rId3"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>`,
  );
  zip.file(
    "ppt/_rels/presentation.xml.rels",
    rels([
      ["rId1", "slide", "slides/slide1.xml"],
      ["rId2", "slide", "slides/slide2.xml"],
      ["rId3", "slide", "slides/slide3.xml"],
    ]),
  );
  zip.file(
    "ppt/theme/theme1.xml",
    `<a:theme ${NS}><a:themeElements><a:clrScheme name="x"><a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:srgbClr val="FFFFFF"/></a:lt1><a:accent1><a:srgbClr val="1F6FEB"/></a:accent1></a:clrScheme></a:themeElements></a:theme>`,
  );
  zip.file(
    "ppt/slideMasters/slideMaster1.xml",
    `<p:sldMaster ${NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FAFAFA"/></a:solidFill></p:bgPr></p:bg><p:spTree/></p:cSld><p:txStyles><p:titleStyle><a:lvl1pPr algn="ctr"><a:defRPr sz="4000" b="1"/></a:lvl1pPr></p:titleStyle><p:bodyStyle><a:lvl1pPr><a:buChar char="•"/><a:defRPr sz="2000"/></a:lvl1pPr></p:bodyStyle></p:txStyles></p:sldMaster>`,
  );
  zip.file(
    "ppt/slideMasters/_rels/slideMaster1.xml.rels",
    rels([["rId1", "theme", "../theme/theme1.xml"]]),
  );
  zip.file(
    "ppt/slideLayouts/slideLayout1.xml",
    `<p:sldLayout ${NS}><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:cNvPr id="2" name="Title"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="200000"/><a:ext cx="8229600" cy="800000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr anchor="b"/><a:p><a:r><a:t>Click to add title</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:cNvPr id="3" name="Band"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="0" y="5000000"/><a:ext cx="9144000" cy="143500"/></a:xfrm><a:solidFill><a:schemeClr val="accent1"/></a:solidFill></p:spPr></p:sp>
    </p:spTree></p:cSld></p:sldLayout>`,
  );
  zip.file(
    "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
    rels([["rId1", "slideMaster", "../slideMasters/slideMaster1.xml"]]),
  );
  const layoutRel = (more: [string, string, string][] = []) =>
    rels([["rId1", "slideLayout", "../slideLayouts/slideLayout1.xml"], ...more]);
  zip.file(
    "ppt/slides/slide1.xml",
    `<p:sld ${NS}><p:cSld><p:spTree>
      <p:sp><p:nvSpPr><p:cNvPr id="2" name="Title 1"/><p:cNvSpPr/><p:nvPr><p:ph type="title"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:bodyPr/><a:p><a:r><a:rPr lang="es"/><a:t>Poza P-01</a:t></a:r></a:p></p:txBody></p:sp>
      <p:sp><p:nvSpPr><p:cNvPr id="3" name="Box"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm rot="5400000"><a:off x="100" y="200"/><a:ext cx="3000" cy="4000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:pPr algn="r"/><a:r><a:rPr sz="1200" i="1"><a:solidFill><a:schemeClr val="accent1"><a:lumMod val="50000"/></a:schemeClr></a:solidFill></a:rPr><a:t>62.8 m³</a:t></a:r><a:br/><a:r><a:t>TR200</a:t></a:r></a:p></p:txBody></p:sp>
      <p:pic><p:nvPicPr><p:cNvPr id="4" name="Pic"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr><a:xfrm><a:off x="10" y="20"/><a:ext cx="30" cy="40"/></a:xfrm></p:spPr></p:pic>
      <p:grpSp><p:nvGrpSpPr><p:cNvPr id="5" name="G"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="1000" y="1000"/><a:ext cx="200" cy="200"/><a:chOff x="0" y="0"/><a:chExt cx="100" cy="100"/></a:xfrm></p:grpSpPr>
        <p:sp><p:nvSpPr><p:cNvPr id="6" name="Dot"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="50" y="50"/><a:ext cx="10" cy="10"/></a:xfrm><a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>
      </p:grpSp>
      <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="7" name="T"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="3000000"/><a:ext cx="4000000" cy="600000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid><a:tr h="300000"><a:tc gridSpan="2"><a:txBody><a:bodyPr/><a:p><a:r><a:t>Caudal</a:t></a:r></a:p></a:txBody></a:tc><a:tc hMerge="1"><a:txBody><a:bodyPr/><a:p/></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
      <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="8" name="C"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="10" cy="10"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart"/></a:graphic></p:graphicFrame>
    </p:spTree></p:cSld></p:sld>`,
  );
  zip.file(
    "ppt/slides/_rels/slide1.xml.rels",
    layoutRel([["rId2", "image", "../media/image1.png"]]),
  );
  zip.file("ppt/media/image1.png", PNG);
  zip.file(
    "ppt/slides/slide2.xml",
    `<p:sld ${NS}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="112233"/></a:solidFill></p:bgPr></p:bg><p:spTree/></p:cSld></p:sld>`,
  );
  zip.file("ppt/slides/_rels/slide2.xml.rels", layoutRel());
  zip.file(
    "ppt/slides/slide3.xml",
    `<p:sld ${NS} show="0"><p:cSld><p:spTree/></p:cSld></p:sld>`,
  );
  zip.file("ppt/slides/_rels/slide3.xml.rels", layoutRel());
  return zip.generateAsync({ type: "uint8array" });
}

describe("reading a PowerPoint deck", () => {
  it("knows the documents the panel shows", () => {
    expect(documentKind("D:/x/Informe final.PDF")).toBe("pdf");
    expect(documentKind("C:\\a\\memoria.docx")).toBe("docx");
    expect(documentKind("/a/avance.pptx")).toBe("pptx");
    expect(documentKind("/a/viejo.ppt")).toBeUndefined();
    expect(documentKind("/a/tabla.xlsx")).toBeUndefined();
  });

  it("resolves a part's relationships", () => {
    expect(resolvePart("ppt/slides/slide1.xml", "../media/a.png")).toBe(
      "ppt/media/a.png",
    );
    expect(resolvePart("ppt/presentation.xml", "/ppt/slides/s.xml")).toBe(
      "ppt/slides/s.xml",
    );
  });

  it("lists the slides in the deck's order, without hidden ones", async () => {
    const result = await readPptx(await deck());
    expect(result.width).toBe(9144000);
    expect(result.height).toBe(5143500);
    expect(result.slides.map((slide) => slide.number)).toEqual([1, 2]);
    // The second in order is slide2.xml, with a background of its own.
    expect(result.slides[0]!.background).toBe("#112233");
    expect(result.slides[1]!.background).toBe("#fafafa");
  });

  it("places a title where its layout does, styled by its master", async () => {
    const slide = (await readPptx(await deck())).slides[1]!;
    const title = slide.items.find(
      (item) => item.kind === "shape" && item.text?.paragraphs[0]?.runs[0]?.text === "Poza P-01",
    );
    expect(title).toMatchObject({ x: 457200, y: 200000, w: 8229600, h: 800000 });
    if (title?.kind !== "shape") throw new Error("no title");
    expect(title.text!.anchor).toBe("bottom");
    expect(title.text!.paragraphs[0]).toMatchObject({ align: "center", size: 40 });
    expect(title.text!.paragraphs[0]!.runs[0]).toMatchObject({ bold: true, size: 40 });
    // The layout's own band shows; its prompt text never does.
    expect(
      slide.items.some((item) => item.kind === "shape" && item.fill === "#1f6feb"),
    ).toBe(true);
    expect(JSON.stringify(slide.items)).not.toContain("Click to add title");
  });

  it("reads text runs, pictures, groups, tables and what it cannot draw", async () => {
    const items = (await readPptx(await deck())).slides[1]!.items;
    const box = items.find(
      (item) => item.kind === "shape" && item.text?.paragraphs[0]?.align === "right",
    );
    if (box?.kind !== "shape") throw new Error("no box");
    expect(box).toMatchObject({ x: 100, y: 200, w: 3000, h: 4000, rotate: 90 });
    expect(box.text!.paragraphs[0]!.runs.map((run) => run.text)).toEqual([
      "62.8 m³",
      "\n",
      "TR200",
    ]);
    expect(box.text!.paragraphs[0]!.runs[0]).toMatchObject({
      size: 12,
      italic: true,
      color: "#103876",
    });
    const picture = items.find((item) => item.kind === "image");
    expect(picture).toMatchObject({ x: 10, y: 20, w: 30, h: 40 });
    if (picture?.kind !== "image") throw new Error("no picture");
    expect(picture.src.startsWith("data:image/png;base64,")).toBe(true);
    // A group's child, mapped from the group's own space to the slide's.
    expect(
      items.find((item) => item.kind === "shape" && item.shape === "ellipse"),
    ).toMatchObject({ x: 1100, y: 1100, w: 20, h: 20, fill: "#ff0000" });
    const table = items.find((item) => item.kind === "table");
    if (table?.kind !== "table") throw new Error("no table");
    expect(table.columns).toEqual([2000000, 2000000]);
    expect(table.rows[0]!.cells).toHaveLength(1);
    expect(table.rows[0]!.cells[0]!.span).toBe(2);
    expect(table.rows[0]!.cells[0]!.text.paragraphs[0]!.runs[0]!.text).toBe("Caudal");
    expect(items.find((item) => item.kind === "unsupported")).toMatchObject({
      label: "Chart",
    });
  });

  it("says when a file is not a deck", async () => {
    const zip = new JSZip();
    zip.file("word/document.xml", "<w:document/>");
    await expect(
      readPptx(await zip.generateAsync({ type: "uint8array" })),
    ).rejects.toThrow("not a PowerPoint deck");
  });
});
