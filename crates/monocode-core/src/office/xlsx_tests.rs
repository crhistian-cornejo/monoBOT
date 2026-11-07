use super::*;
use crate::office::fixtures::{package, REL, RELS_NS};
use crate::office::xml::parse_xml;

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

fn en() -> Locale {
    Locale::en()
}

fn lob_workbook() -> Vec<u8> {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets>
  <sheet name="LOB" sheetId="1" r:id="rId1"/>
  <sheet name="Obs 28 detalle" sheetId="2" r:id="rId2"/>
  <sheet name="Oculta" sheetId="3" state="hidden" r:id="rId3"/>
</sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}">
  <Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/>
  <Relationship Id="rId2" Type="{REL}/worksheet" Target="worksheets/sheet2.xml"/>
  <Relationship Id="rId3" Type="{REL}/worksheet" Target="worksheets/sheet3.xml"/>
  <Relationship Id="rId4" Type="{REL}/styles" Target="styles.xml"/>
  <Relationship Id="rId5" Type="{REL}/sharedStrings" Target="sharedStrings.xml"/>
</Relationships>"#
    );
    let styles = format!(
        r#"<styleSheet xmlns="{MAIN}">
  <numFmts count="1"><numFmt numFmtId="164" formatCode="dd/mm/yyyy"/></numFmts>
  <fonts count="3">
    <font><sz val="11"/><name val="Calibri"/></font>
    <font><b/><sz val="11"/><color rgb="FFFFFFFF"/><name val="Calibri"/></font>
    <font><sz val="11"/><color rgb="FFC00000"/><name val="Calibri"/></font>
  </fonts>
  <fills count="3">
    <fill><patternFill patternType="none"/></fill>
    <fill><patternFill patternType="gray125"/></fill>
    <fill><patternFill patternType="solid"><fgColor rgb="FFED7D31"/></patternFill></fill>
  </fills>
  <borders count="2">
    <border><left/><right/><top/><bottom/></border>
    <border><left/><right/><top/><bottom style="medium"><color rgb="FF000000"/></bottom></border>
  </borders>
  <cellXfs count="5">
    <xf numFmtId="0" fontId="0" fillId="0" borderId="0"/>
    <xf numFmtId="0" fontId="1" fillId="2" borderId="0"><alignment horizontal="center" vertical="center" wrapText="1"/></xf>
    <xf numFmtId="3" fontId="0" fillId="0" borderId="0"/>
    <xf numFmtId="164" fontId="0" fillId="0" borderId="0"/>
    <xf numFmtId="0" fontId="2" fillId="0" borderId="1"/>
  </cellXfs>
</styleSheet>"#
    );
    let strings = format!(r#"<sst xmlns="{MAIN}"><si><t>Qué pide SENACE</t></si></sst>"#);
    let sheet1 = format!(
        r#"<worksheet xmlns="{MAIN}">
  <sheetViews><sheetView showGridLines="0"><pane xSplit="1" ySplit="4" topLeftCell="B5" state="frozen"/></sheetView></sheetViews>
  <cols><col min="2" max="2" width="30" customWidth="1"/><col min="7" max="7" width="9" hidden="1"/></cols>
  <sheetData>
    <row r="1"><c r="A1" s="1" t="s"><v>0</v></c></row>
    <row r="2">
      <c r="B2" s="2"><v>1066</v></c>
      <c r="C2"><f>B2*2</f><v>2132</v></c>
      <c r="D2"><f>A1&amp;"x"</f></c>
      <c r="E2" t="e"><v>#N/A</v></c>
      <c r="F2" s="3"><v>46331</v></c>
    </row>
    <row r="3"><c r="B3" s="4" t="inlineStr"><is><t>rojo</t></is></c></row>
    <row r="5" ht="30" customHeight="1"><c r="A5"><v>28</v></c></row>
    <row r="6"><c r="A6" t="str"><v>cae dentro del merge</v></c></row>
  </sheetData>
  <mergeCells count="1"><mergeCell ref="A5:A8"/></mergeCells>
</worksheet>"#
    );
    let sheet2 = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>otra</t></is></c></row></sheetData></worksheet>"#
    );
    let sheet3 = format!(r#"<worksheet xmlns="{MAIN}"><sheetData/></worksheet>"#);
    package(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/sharedStrings.xml", strings.as_bytes()),
        ("xl/worksheets/sheet1.xml", sheet1.as_bytes()),
        ("xl/worksheets/sheet2.xml", sheet2.as_bytes()),
        ("xl/worksheets/sheet3.xml", sheet3.as_bytes()),
    ])
}

fn cell<'a>(sheet: &'a Sheet, address: &str) -> Option<&'a SheetCell> {
    let (row, col) = parse_address(address)?;
    sheet.cells.iter().find(|c| c.row == row && c.col == col)
}

#[test]
fn names_columns_as_excel_does() {
    let names: Vec<String> = [0, 25, 26, 51, 52, 701, 702].map(column_name).into();
    assert_eq!(names, ["A", "Z", "AA", "AZ", "BA", "ZZ", "AAA"]);
    assert_eq!(cell_address(4, 1), "B5");
    assert_eq!(parse_address("$AB$12"), Some((11, 27)));
    assert_eq!(parse_address("12"), None);
    assert_eq!(parse_address("A0"), None);
}

#[test]
fn routes_spreadsheet_files_to_the_viewer() {
    assert!(is_spreadsheet_path("C:\\obra\\LOB_SENACE.xlsx"));
    assert!(is_spreadsheet_path("/tmp/data.CSV"));
    assert!(is_spreadsheet_path("/tmp/old.xls"));
    assert!(!is_spreadsheet_path("/tmp/notes.md"));
    // CSV stays editable text in the editor.
    assert!(!is_workbook_path("/tmp/data.csv"));
    assert!(is_workbook_path("/tmp/LOB.XLSX"));
    assert!(is_legacy_xls("/x/old.XLS"));
}

#[test]
fn reads_a_theme_with_light_colours_first() {
    let xml = parse_xml(
        r#"<a:dk1><a:sysClr val="windowText" lastClr="111111"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FEFEFE"/></a:lt1>
        <a:dk2><a:srgbClr val="222222"/></a:dk2><a:lt2><a:srgbClr val="EEEEEE"/></a:lt2><a:accent1><a:srgbClr val="FF0000"/></a:accent1>"#,
    );
    let theme = theme_colors(Some(&xml));
    assert_eq!(
        theme[..5],
        ["#fefefe", "#111111", "#eeeeee", "#222222", "#ff0000"]
    );
    // Missing entries keep Office's.
    assert_eq!(theme[5], "#ed7d31");
    assert_eq!(theme_colors(None)[4], "#4472c4");
}

#[test]
fn tints_toward_white_and_black() {
    assert_eq!(apply_tint("#000000", 0.5), "#808080");
    assert_eq!(apply_tint("#ffffff", -0.5), "#808080");
    let theme = theme_colors(None);
    let tinted = XColor {
        theme: Some(5),
        tint: Some(0.4),
        ..XColor::default()
    };
    assert_eq!(
        excel_color(Some(&tinted), &theme),
        Some(apply_tint("#ed7d31", 0.4))
    );
    let argb = XColor {
        argb: Some("FFED7D31".into()),
        ..XColor::default()
    };
    assert_eq!(excel_color(Some(&argb), &[]), Some("#ed7d31".to_string()));
    let transparent = XColor {
        argb: Some("00ED7D31".into()),
        ..XColor::default()
    };
    assert_eq!(excel_color(Some(&transparent), &theme), None);
}

#[test]
fn picks_text_that_reads_on_a_fill() {
    assert_eq!(readable_on("#ffff00"), "#000000");
    assert_eq!(readable_on("#1f3864"), "#ffffff");
}

#[test]
fn splits_quoted_fields_with_delimiters_and_line_breaks() {
    assert_eq!(
        parse_delimited("a,\"b,c\",\"d \"\"e\"\"\"\r\n1,\"2\n3\",\n", ','),
        [vec!["a", "b,c", "d \"e\""], vec!["1", "2\n3", ""]]
    );
}

#[test]
fn sniffs_semicolons_and_tabs() {
    assert_eq!(sniff_delimiter("a;b;c\n1;2;3", "x.csv"), ';');
    assert_eq!(sniff_delimiter("a,b", "x.tsv"), '\t');
    assert_eq!(sniff_delimiter("single", "x.csv"), ',');
}

#[test]
fn builds_a_sheet_from_csv_with_numbers_leaning_right() {
    let Workbook { sheets } = csv_workbook("\u{feff}Item,Volumen\na,340000\n", "/p/obs.csv");
    assert_eq!(sheets[0].name, "obs");
    let cell = |row, col, text: &str, numeric| SheetCell {
        row,
        col,
        text: text.into(),
        input: text.into(),
        numeric,
        ..SheetCell::default()
    };
    assert_eq!(
        sheets[0].cells,
        [
            cell(0, 0, "Item", false),
            cell(0, 1, "Volumen", false),
            cell(1, 0, "a", false),
            cell(1, 1, "340000", true),
        ]
    );
    assert!(sheets[0].row_count >= 2);
    assert_eq!(sheets[0].col_widths.len(), sheets[0].col_count);
}

#[test]
fn reads_values_formulas_styles_merges_and_layout() {
    let Workbook { sheets } =
        parse_workbook(lob_workbook(), "C:/obra/LOB.xlsx", &en()).expect("a workbook");
    let names: Vec<&str> = sheets.iter().map(|s| s.name.as_str()).collect();
    // The hidden sheet is read (a chart may draw from it) but not returned.
    assert_eq!(names, ["LOB", "Obs 28 detalle"]);
    let lob = &sheets[0];

    let a1 = cell(lob, "A1").expect("A1");
    assert_eq!(a1.text, "Qué pide SENACE");
    let style = a1.style.as_ref().expect("a style");
    assert_eq!(style.fill.as_deref(), Some("#ed7d31"));
    assert!(style.bold && style.wrap);
    assert_eq!(style.align, Some(HAlign::Center));
    assert_eq!(style.valign, Some(VAlign::Middle));
    // White on a fill is Excel's automatic colour: the viewer picks it.
    assert_eq!(style.color, None);

    let b2 = cell(lob, "B2").expect("B2");
    assert_eq!(
        (b2.text.as_str(), b2.input.as_str(), b2.numeric),
        ("1,066", "1066", true)
    );
    assert_eq!(b2.value, Some(1066.0));

    let c2 = cell(lob, "C2").expect("C2");
    assert_eq!((c2.text.as_str(), c2.input.as_str()), ("2132", "=B2*2"));
    assert!(!c2.uncalculated);

    let d2 = cell(lob, "D2").expect("D2");
    assert_eq!(
        (d2.text.as_str(), d2.input.as_str()),
        ("=A1&\"x\"", "=A1&\"x\"")
    );
    assert!(d2.uncalculated);

    let e2 = cell(lob, "E2").expect("E2");
    assert_eq!(e2.text, "#N/A");
    assert!(e2.error);

    let f2 = cell(lob, "F2").expect("F2");
    assert_eq!(
        (f2.text.as_str(), f2.input.as_str()),
        ("05/11/2026", "2026-11-05")
    );
    assert!(f2.numeric);

    let b3 = cell(lob, "B3").expect("B3");
    assert_eq!(b3.text, "rojo");
    let style = b3.style.as_ref().expect("a style");
    assert_eq!(style.color.as_deref(), Some("#c00000"));
    let bottom = style.bottom.as_ref().expect("a bottom border");
    assert_eq!(
        (bottom.width, bottom.style, bottom.color.as_deref()),
        (2, BorderStyle::Solid, None)
    );

    let a5 = cell(lob, "A5").expect("A5");
    assert_eq!(
        (a5.text.as_str(), a5.row_span, a5.col_span),
        ("28", Some(4), Some(1))
    );
    // Cells under a merge, other than its corner, are gone.
    assert!(cell(lob, "A6").is_none());

    assert_eq!(lob.col_widths[1], column_pixels(30.0));
    assert_eq!(lob.col_widths[6], 0.0);
    assert_eq!(lob.row_heights[4], 40.0);
    assert_eq!((lob.frozen_rows, lob.frozen_cols), (4, 1));
    assert!(!lob.gridlines);
    assert!(!lob.truncated);
    assert!(lob.row_count >= 40 && lob.col_count >= 12);
    assert_eq!(lob.col_widths.len(), lob.col_count);
    assert_eq!(lob.row_heights.len(), lob.row_count);
    assert_eq!(sheets[1].cells[0].text, "otra");
    assert!(sheets[1].gridlines);
}

#[test]
fn prints_in_the_locale_asked_for() {
    let Workbook { sheets } =
        parse_workbook(lob_workbook(), "LOB.xlsx", &Locale::es()).expect("a workbook");
    assert_eq!(cell(&sheets[0], "B2").expect("B2").text, "1066");
    assert_eq!(cell(&sheets[0], "F2").expect("F2").text, "05/11/2026");
}

#[test]
fn shares_formulas_and_reads_rich_and_escaped_strings() {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><workbookPr date1904="1"/><sheets><sheet name="Hoja" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/worksheet" Target="/xl/worksheets/s.xml"/></Relationships>"#
    );
    let strings = format!(
        r#"<sst xmlns="{MAIN}"><si><r><t>Rojo </t></r><r><rPr><b/></rPr><t>y azul</t></r><rPh><t>fonetica</t></rPh></si><si><t>a &amp; b</t></si></sst>"#
    );
    let sheet = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData>
  <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1"><v>10</v></c><c r="D1" t="b"><v>1</v></c></row>
  <row r="2"><c r="C2"><f t="shared" ref="C2:C4" si="0">C1*$A$1+SUM(C1:D1)</f><v>1</v></c></row>
  <row r="3"><c r="C3"><f t="shared" si="0"/><v>2</v></c></row>
  <row r="4"><c r="C4"><f t="shared" si="0"/><v>3</v></c></row>
</sheetData></worksheet>"#
    );
    let bytes = package(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/sharedStrings.xml", strings.as_bytes()),
        ("xl/worksheets/s.xml", sheet.as_bytes()),
    ]);
    let Workbook { sheets } = parse_workbook(bytes, "x.xlsm", &en()).expect("a workbook");
    let hoja = &sheets[0];
    assert_eq!(cell(hoja, "A1").expect("A1").text, "Rojo y azul");
    assert_eq!(cell(hoja, "B1").expect("B1").text, "a & b");
    assert_eq!(cell(hoja, "D1").expect("D1").text, "TRUE");
    assert_eq!(cell(hoja, "C2").expect("C2").input, "=C1*$A$1+SUM(C1:D1)");
    // The shared formula shifts its relative references and keeps the absolute ones.
    assert_eq!(cell(hoja, "C3").expect("C3").input, "=C2*$A$1+SUM(C2:D2)");
    assert_eq!(cell(hoja, "C4").expect("C4").input, "=C3*$A$1+SUM(C3:D3)");
}

#[test]
fn reads_dates_in_the_1904_system() {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><workbookPr date1904="1"/><sheets><sheet name="Hoja" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/worksheet" Target="s.xml"/></Relationships>"#
    );
    let sheet = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1" s="0"><v>44869</v></c></row></sheetData></worksheet>"#
    );
    let styles = format!(
        r#"<styleSheet xmlns="{MAIN}"><cellXfs count="1"><xf numFmtId="14"/></cellXfs></styleSheet>"#
    );
    let bytes = package(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
        ("xl/styles.xml", styles.as_bytes()),
        ("xl/s.xml", sheet.as_bytes()),
    ]);
    let Workbook { sheets } = parse_workbook(bytes, "x.xlsx", &en()).expect("a workbook");
    // 44869 days from 1904 is 2026-11-05; the 1900 system would say 2022.
    assert_eq!(cell(&sheets[0], "A1").expect("A1").input, "2026-11-05");
}

#[test]
fn explains_files_it_cannot_read() {
    let old = parse_workbook(vec![1, 2, 3], "/x/old.xls", &en());
    assert!(old
        .expect_err("refused")
        .contains("old binary Excel format"));
    let broken = parse_workbook(b"not a zip".to_vec(), "/x/broken.xlsx", &en());
    assert!(broken
        .expect_err("refused")
        .starts_with("This file could not be read as a workbook"));
    let empty = package(&[("hello.txt", b"hi")]);
    assert!(parse_workbook(empty, "/x/e.xlsx", &en()).is_err());
}

#[test]
fn refuses_a_workbook_with_nothing_to_show() {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="A" sheetId="1" state="veryHidden" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/worksheet" Target="s.xml"/></Relationships>"#
    );
    let bytes = package(&[
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
    ]);
    assert_eq!(
        parse_workbook(bytes, "x.xlsx", &en()),
        Err("This workbook has no visible sheets.".to_string())
    );
}

#[test]
fn shifts_references_in_shared_formulas() {
    assert_eq!(shift_formula("A1+B$2+$C3+$D$4", 1, 1), "B2+C$2+$C4+$D$4");
    assert_eq!(shift_formula("SUM(A1:A3)", 2, 0), "SUM(A3:A5)");
    assert_eq!(shift_formula("LOG10(A1)&\"A1\"", 1, 0), "LOG10(A2)&\"A1\"");
    assert_eq!(
        shift_formula("'Hoja 1'!A1+Hoja2!B2", 1, 1),
        "'Hoja 1'!B2+Hoja2!C3"
    );
}

#[test]
fn serializes_the_way_the_typescript_model_does() {
    let Workbook { sheets } =
        parse_workbook(lob_workbook(), "LOB.xlsx", &en()).expect("a workbook");
    let json = serde_json::to_value(&sheets[0]).expect("json");
    assert_eq!(json["frozenRows"], 4);
    assert_eq!(json["colWidths"][6], 0.0);
    let a5 = json["cells"]
        .as_array()
        .expect("cells")
        .iter()
        .find(|c| c["text"] == "28")
        .expect("A5");
    assert_eq!(a5["rowSpan"], 4);
    assert!(a5.get("numeric").is_some_and(|n| n == true));
    assert!(a5.get("error").is_none());
    let round: Sheet = serde_json::from_value(json).expect("round trip");
    assert_eq!(round, sheets[0]);
}
