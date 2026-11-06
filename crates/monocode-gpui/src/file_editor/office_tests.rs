//! The office viewers in the file pane, driven through a real `FileEditor`
//! on fixture files written here with the zip crate.
use super::*;
use gpui::{Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, size};
use std::io::Write;
use zip::write::SimpleFileOptions;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const A: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
const XDR: &str =
    r#"xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing""#;
const C: &str = r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart""#;

fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(bytes).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD
        .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==")
        .unwrap()
}

fn rels(entries: &[(&str, &str, &str)]) -> String {
    let items: String = entries
        .iter()
        .map(|(id, kind, target)| {
            format!("<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{target}\"/>")
        })
        .collect();
    format!("<Relationships xmlns=\"{RELS_NS}\">{items}</Relationships>")
}

fn marker(tag: &str, col: u32, row: u32) -> String {
    format!(
        "<xdr:{tag}><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:{tag}>"
    )
}

/// Two sheets; the first has a styled header, a merge, a frozen row, a
/// column chart and a picture.
fn workbook() -> Vec<u8> {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="Datos" sheetId="1" r:id="rId1"/><sheet name="Resumen" sheetId="2" r:id="rId2"/></sheets></workbook>"#
    );
    let workbook_rels = rels(&[
        ("rId1", "worksheet", "worksheets/sheet1.xml"),
        ("rId2", "worksheet", "worksheets/sheet2.xml"),
        ("rId3", "styles", "styles.xml"),
    ]);
    let styles = format!(
        r#"<styleSheet xmlns="{MAIN}"><fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFED7D31"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/></border></borders><cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/><xf numFmtId="0" fontId="1" fillId="2" borderId="0"/></cellXfs></styleSheet>"#
    );
    let sheet1 = format!(
        r#"<worksheet xmlns="{MAIN}" xmlns:r="{REL}"><sheetViews><sheetView><pane ySplit="1" topLeftCell="A2" state="frozen"/></sheetView></sheetViews><sheetData><row r="1"><c r="A1" s="1" t="inlineStr"><is><t>Producto</t></is></c><c r="B1" s="1" t="inlineStr"><is><t>Ventas</t></is></c><c r="D1" t="inlineStr"><is><t>Total</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>Norte</t></is></c><c r="B2"><v>1200</v></c></row><row r="3"><c r="A3" t="inlineStr"><is><t>Sur</t></is></c><c r="B3"><v>800</v></c><c r="C3"><f>B2+B3</f><v>2000</v></c></row></sheetData><mergeCells count="1"><mergeCell ref="D1:E1"/></mergeCells><drawing r:id="rId1"/></worksheet>"#
    );
    let sheet1_rels = rels(&[("rId1", "drawing", "../drawings/drawing1.xml")]);
    let sheet2 = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Otra hoja</t></is></c></row></sheetData></worksheet>"#
    );
    let drawing = format!(
        r#"<xdr:wsDr {XDR} {A} xmlns:r="{REL}"><xdr:twoCellAnchor>{}{}<xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="3" name="Grafico"/></xdr:nvGraphicFramePr><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId2"/></a:graphicData></a:graphic></xdr:graphicFrame></xdr:twoCellAnchor><xdr:twoCellAnchor>{}{}<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="Plano" descr="Planta"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic></xdr:twoCellAnchor></xdr:wsDr>"#,
        marker("from", 3, 4),
        marker("to", 8, 14),
        marker("from", 3, 16),
        marker("to", 6, 20),
    );
    let drawing_rels = rels(&[
        ("rId1", "image", "../media/image1.png"),
        ("rId2", "chart", "../charts/chart1.xml"),
    ]);
    let chart = format!(
        r#"<c:chartSpace {C}><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:f>Datos!$B$1</c:f></c:strRef></c:tx><c:cat><c:strRef><c:f>Datos!$A$2:$A$3</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>Datos!$B$2:$B$3</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
    );
    let image = png();
    package(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        (
            "xl/worksheets/_rels/sheet1.xml.rels",
            sheet1_rels.as_bytes(),
        ),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/drawings/drawing1.xml", drawing.as_bytes()),
        (
            "xl/drawings/_rels/drawing1.xml.rels",
            drawing_rels.as_bytes(),
        ),
        ("xl/charts/chart1.xml", chart.as_bytes()),
        ("xl/media/image1.png", image.as_slice()),
    ])
}

fn document() -> Vec<u8> {
    let w = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
    let body = format!(
        r#"<w:document {w}><w:body>
  <w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:b/><w:sz w:val="40"/></w:rPr><w:t>Memoria descriptiva</w:t></w:r></w:p>
  <w:p><w:r><w:t xml:space="preserve">Poza </w:t></w:r><w:r><w:rPr><w:i/><w:color w:val="C00000"/></w:rPr><w:t>P-01</w:t></w:r><w:hyperlink r:id="rId9"><w:r><w:t> SENACE</w:t></w:r></w:hyperlink></w:p>
  <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>Primero</w:t></w:r></w:p>
  <w:p><w:r><w:drawing><wp:inline><wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Imagen 1" descr="Plano"/><a:graphic><a:graphicData><a:blip r:embed="rId5"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>
  <w:tbl><w:tblGrid><w:gridCol w:w="2000"/><w:gridCol w:w="2000"/></w:tblGrid>
    <w:tr><w:tc><w:p><w:r><w:t>Caudal</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Unidad</w:t></w:r></w:p></w:tc></w:tr>
    <w:tr><w:tc><w:p><w:r><w:t>99</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>L/s</w:t></w:r></w:p></w:tc></w:tr>
  </w:tbl>
  <w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:r><w:t>Segunda página</w:t></w:r></w:p>
  <w:sectPr><w:headerReference w:type="default" r:id="rId7"/><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440"/></w:sectPr>
</w:body></w:document>"#
    );
    let numbering = r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#;
    let styles = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults></w:styles>"#;
    let mut document_rels = rels(&[
        ("rId1", "styles", "styles.xml"),
        ("rId2", "numbering", "numbering.xml"),
        ("rId5", "image", "media/plano.png"),
        ("rId7", "header", "header1.xml"),
    ]);
    document_rels = document_rels.replace(
        "</Relationships>",
        &format!("<Relationship Id=\"rId9\" Type=\"{REL}/hyperlink\" Target=\"https://www.senace.gob.pe/\" TargetMode=\"External\"/></Relationships>"),
    );
    let header = format!(r#"<w:hdr {w}><w:p><w:r><w:t>Informe 28</w:t></w:r></w:p></w:hdr>"#);
    let image = png();
    package(&[
        ("word/document.xml", body.as_bytes()),
        ("word/_rels/document.xml.rels", document_rels.as_bytes()),
        ("word/styles.xml", styles.as_bytes()),
        ("word/numbering.xml", numbering.as_bytes()),
        ("word/header1.xml", header.as_bytes()),
        ("word/media/plano.png", image.as_slice()),
    ])
}

fn deck() -> Vec<u8> {
    let ns = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main""#;
    let presentation = format!(
        r#"<p:presentation {ns}><p:sldIdLst><p:sldId id="256" r:id="rId1"/><p:sldId id="257" r:id="rId2"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#
    );
    let presentation_rels = rels(&[
        ("rId1", "slide", "slides/slide1.xml"),
        ("rId2", "slide", "slides/slide2.xml"),
    ]);
    let slide1 = format!(
        r#"<p:sld {ns}><p:cSld><p:bg><p:bgPr><a:solidFill><a:srgbClr val="FAFAFA"/></a:solidFill></p:bgPr></p:bg><p:spTree>
  <p:sp><p:nvSpPr><p:cNvPr id="2" name="Titulo"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="457200" y="200000"/><a:ext cx="8229600" cy="800000"/></a:xfrm></p:spPr><p:txBody><a:bodyPr/><a:p><a:r><a:rPr sz="3200" b="1"/><a:t>Poza P-01</a:t></a:r></a:p></p:txBody></p:sp>
  <p:sp><p:nvSpPr><p:cNvPr id="3" name="Punto"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="100000" y="2000000"/><a:ext cx="500000" cy="500000"/></a:xfrm><a:prstGeom prst="ellipse"/><a:solidFill><a:srgbClr val="FF0000"/></a:solidFill></p:spPr></p:sp>
  <p:pic><p:nvPicPr><p:cNvPr id="4" name="Pic"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed="rId2"/></p:blipFill><p:spPr><a:xfrm><a:off x="5000000" y="1500000"/><a:ext cx="1000000" cy="1000000"/></a:xfrm></p:spPr></p:pic>
  <p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="7" name="T"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="3000000"/><a:ext cx="4000000" cy="600000"/></p:xfrm><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/table"><a:tbl><a:tblGrid><a:gridCol w="2000000"/><a:gridCol w="2000000"/></a:tblGrid><a:tr h="300000"><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>Caudal</a:t></a:r></a:p></a:txBody></a:tc><a:tc><a:txBody><a:bodyPr/><a:p><a:r><a:t>99</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>
</p:spTree></p:cSld></p:sld>"#
    );
    let slide2 = format!(r#"<p:sld {ns}><p:cSld><p:spTree/></p:cSld></p:sld>"#);
    let slide1_rels = rels(&[("rId2", "image", "../media/image1.png")]);
    let image = png();
    package(&[
        ("ppt/presentation.xml", presentation.as_bytes()),
        (
            "ppt/_rels/presentation.xml.rels",
            presentation_rels.as_bytes(),
        ),
        ("ppt/slides/slide1.xml", slide1.as_bytes()),
        ("ppt/slides/_rels/slide1.xml.rels", slide1_rels.as_bytes()),
        ("ppt/slides/slide2.xml", slide2.as_bytes()),
        ("ppt/media/image1.png", image.as_slice()),
    ])
}

fn scratch(name: &str, bytes: &[u8]) -> String {
    let dir = std::env::temp_dir().join(format!("monocode-office-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path.to_string_lossy().into_owned()
}

fn open(
    cx: &mut TestAppContext,
    path: &str,
    mode: &str,
) -> (gpui::WindowHandle<FileEditor>, VisualTestContext) {
    let file = path.to_string();
    let mode = mode.to_string();
    let window = cx.update(|cx| {
        cx.set_global(crate::settings::Preferences {
            file_view_mode: mode,
            ..Default::default()
        });
        input::bind(cx);
        bind(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(900.), px(640.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                let git = cx.new(|cx| {
                    ProjectGit::new(std::env::temp_dir().to_string_lossy().into_owned(), cx)
                });
                cx.new(|cx| FileEditor::new(file, git, cx))
            },
        )
        .unwrap()
    });
    let mut visual = VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    // Decoding pictures and measuring the view take a second pass each.
    for _ in 0..3 {
        window.update(&mut visual, |_, _, cx| cx.notify()).unwrap();
        visual.run_until_parked();
    }
    (window, visual)
}

fn shown(cx: &mut VisualTestContext, selector: &'static str) -> bool {
    cx.debug_bounds(selector).is_some()
}

#[gpui::test]
fn a_workbook_shows_its_tabs_cells_chart_and_picture(cx: &mut TestAppContext) {
    let path = scratch("datos.xlsx", &workbook());
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, _| {
            assert!(e.office.is_some(), "{:?}", e.error);
            assert!(e.error.is_none());
        })
        .unwrap();
    // Sheet tabs.
    assert!(shown(&mut cx, "sheet-tab-0") && shown(&mut cx, "sheet-tab-1"));
    // Name box, formula bar and the grid.
    assert!(shown(&mut cx, "sheet-name-box") && shown(&mut cx, "sheet-formula-input"));
    assert!(shown(&mut cx, "sheet-grid"));
    // Cell text, including the merged cell and a formula's result.
    for cell in [
        "sheet-cell-0-0",
        "sheet-cell-0-1",
        "sheet-cell-1-0",
        "sheet-cell-2-2",
        "sheet-cell-0-3",
    ] {
        assert!(cx.debug_bounds(cell).is_some(), "{cell}");
    }
    // The chart and the picture over the grid.
    assert!(shown(&mut cx, "sheet-drawing-0"), "chart");
    assert!(shown(&mut cx, "sheet-drawing-1"), "picture");
    // The other sheet's cells are not drawn until its tab is picked.
    assert!(cx.debug_bounds("sheet-cell-0-0").is_some());
    let tab = cx.debug_bounds("sheet-tab-1").unwrap().center();
    cx.simulate_click(tab, gpui::Modifiers::none());
    cx.run_until_parked();
    assert!(!shown(&mut cx, "sheet-drawing-0"));
    assert!(shown(&mut cx, "sheet-cell-0-0"));
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn a_big_sheet_only_builds_the_cells_in_view(cx: &mut TestAppContext) {
    let mut csv = String::new();
    for row in 0..500 {
        let line: Vec<String> = (0..500).map(|col| format!("r{row}c{col}")).collect();
        csv.push_str(&line.join(","));
        csv.push('\n');
    }
    let path = scratch("grande.csv", csv.as_bytes());
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, _| {
            assert!(e.office.is_some(), "{:?}", e.error)
        })
        .unwrap();
    assert!(shown(&mut cx, "sheet-grid"));
    assert!(shown(&mut cx, "sheet-cell-0-0"));
    assert!(!shown(&mut cx, "sheet-cell-499-499"));
    assert!(!shown(&mut cx, "sheet-cell-300-0"));
    // Scrolling far down brings the last rows in and drops the first.
    window
        .update(&mut cx, |e, _, cx| {
            let sheet = e
                .office
                .clone()
                .unwrap()
                .downcast::<spreadsheet::SpreadsheetView>()
                .ok()
                .unwrap();
            sheet.update(cx, |sheet, cx| sheet.scroll_by(0., 1.0e9, cx));
        })
        .unwrap();
    cx.run_until_parked();
    assert!(!shown(&mut cx, "sheet-cell-0-0"));
    assert!(shown(&mut cx, "sheet-cell-499-0"));
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn a_csv_opens_as_a_sheet_with_the_text_a_toggle_away(cx: &mut TestAppContext) {
    let path = scratch("tabla.csv", b"a;b\n1;2\n");
    let (window, mut cx) = open(cx, &path, "source");
    window
        .update(&mut cx, |e, _, _| {
            assert!(!e.preview);
            assert!(e.document.is_some());
        })
        .unwrap();
    assert!(!shown(&mut cx, "sheet-grid"));
    window
        .update(&mut cx, |e, w, cx| e.toggle_preview(w, cx))
        .unwrap();
    cx.run_until_parked();
    assert!(shown(&mut cx, "sheet-grid"));
    assert!(shown(&mut cx, "sheet-cell-1-0"));
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn the_old_excel_format_is_refused_with_a_message(cx: &mut TestAppContext) {
    let path = scratch("viejo.xls", b"\xd0\xcf\x11\xe0 old");
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, _| {
            assert!(e.office.is_none());
            assert!(
                e.error
                    .as_deref()
                    .is_some_and(|m| m.contains("old binary Excel format")),
                "{:?}",
                e.error
            );
        })
        .unwrap();
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn a_broken_file_shows_the_reading_error(cx: &mut TestAppContext) {
    let path = scratch("roto.docx", b"not a zip");
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, _| {
            assert!(e.office.is_none());
            assert!(e.error.is_some());
        })
        .unwrap();
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn a_word_document_shows_pages_with_its_blocks_and_a_page_count(cx: &mut TestAppContext) {
    let path = scratch("memoria.docx", &document());
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, cx| {
            assert!(e.office.is_some(), "{:?}", e.error);
            let Ok(view) = e.office.clone().unwrap().downcast::<word::DocumentView>() else {
                panic!("a document view")
            };
            // The page break makes two pages.
            assert_eq!(view.read(cx).page_count(), 2);
        })
        .unwrap();
    assert!(shown(&mut cx, "office-count"));
    assert!(shown(&mut cx, "docx-page-0"));
    // Heading, runs and link, list item, picture and table are all blocks.
    for block in [
        "docx-block-0",
        "docx-block-1",
        "docx-block-2",
        "docx-block-3",
        "docx-block-4",
    ] {
        assert!(cx.debug_bounds(block).is_some(), "{block}");
    }
    assert!(shown(&mut cx, "docx-image-0"), "the inline picture");
    std::fs::remove_file(path).ok();
}

#[gpui::test]
fn a_deck_shows_its_slides_scaled_to_fit_with_their_items(cx: &mut TestAppContext) {
    let path = scratch("avance.pptx", &deck());
    let (window, mut cx) = open(cx, &path, "preview");
    window
        .update(&mut cx, |e, _, cx| {
            assert!(e.office.is_some(), "{:?}", e.error);
            let Ok(view) = e.office.clone().unwrap().downcast::<deck::DeckView>() else {
                panic!("a deck view")
            };
            assert_eq!(view.read(cx).slide_count(), 2);
        })
        .unwrap();
    assert!(shown(&mut cx, "office-count"));
    let slide = cx.debug_bounds("slide-0").expect("the first slide");
    // The slide is as wide as the panel less its gutters, with the deck's 16:9.
    let ratio = slide.size.width.as_f32() / slide.size.height.as_f32();
    assert!((ratio - 16. / 9.).abs() < 0.05, "{ratio}");
    for item in [
        "slide-0-item-0",
        "slide-0-item-1",
        "slide-0-item-2",
        "slide-0-item-3",
    ] {
        assert!(cx.debug_bounds(item).is_some(), "{item}");
    }
    std::fs::remove_file(path).ok();
}

/// `MONOCODE_OFFICE_DIR=<folder>` with `--ignored`: every office file in the
/// folder opens and draws without panicking. The files are only read.
#[gpui::test]
#[ignore = "reads the files in MONOCODE_OFFICE_DIR"]
fn real_office_files_open_and_draw(cx: &mut TestAppContext) {
    let Ok(dir) = std::env::var("MONOCODE_OFFICE_DIR") else {
        return;
    };
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().to_string_lossy().into_owned())
        .filter(|path| office::kind_of(path).is_some())
        .filter(|path| std::fs::metadata(path).is_ok_and(|m| m.len() < 30 * 1024 * 1024))
        .collect();
    files.sort();
    for path in files {
        let started = std::time::Instant::now();
        let (window, mut cx) = open(cx, &path, "preview");
        let outcome = window
            .update(&mut cx, |e, _, _| (e.office.is_some(), e.error.clone()))
            .unwrap();
        println!(
            "{} -> {} ({:?}) in {:?}",
            path,
            if outcome.0 { "shown" } else { "error" },
            outcome.1,
            started.elapsed()
        );
    }
}
