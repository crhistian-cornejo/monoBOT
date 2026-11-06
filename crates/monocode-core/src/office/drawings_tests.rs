use super::*;
use crate::office::fixtures::{package, png, REL, RELS_NS};
use crate::office::numfmt::Locale;
use crate::office::xlsx::{column_pixels, parse_workbook, theme_colors, SheetCell, Workbook};

const C: &str = r#"xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart""#;
const A: &str = r#"xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main""#;
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const XDR: &str =
    r#"xmlns:xdr="http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing""#;

fn theme() -> Vec<String> {
    theme_colors(None)
}

fn no_ranges(_: &str) -> RangeValues {
    RangeValues::default()
}

#[test]
fn reads_a_cached_column_chart_title_series_colours_axes() {
    let xml = format!(
        r#"<c:chartSpace {C} {A}><c:chart>
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
      </c:plotArea><c:legend/></c:chart></c:chartSpace>"#
    );
    let chart = parse_chart(&xml, &theme(), &no_ranges);
    assert_eq!(chart.title.as_deref(), Some("Balance"));
    assert_eq!(chart.categories, ["a", "b"]);
    assert!(chart.legend);
    assert_eq!(chart.x_title.as_deref(), Some("Ítem"));
    assert_eq!(chart.y_title.as_deref(), Some("m³"));
    assert_eq!(
        chart.series,
        [
            ChartSeries {
                kind: ChartKind::Column,
                name: "Oferta".into(),
                values: vec![Some(340_000.0), Some(1066.0)],
                x_values: None,
                color: Some("#ed7d31".into()),
                point_colors: None,
            },
            // A missing point stays empty; the colour falls back to the theme.
            ChartSeries {
                kind: ChartKind::Column,
                name: "Demanda".into(),
                values: vec![None, Some(500.0)],
                x_values: None,
                color: Some(theme()[5].clone()),
                point_colors: None,
            },
        ]
    );
}

fn datos_sheet() -> Sheet {
    let text_cell = |row, col, text: &str, value: Option<f64>| SheetCell {
        row,
        col,
        text: text.into(),
        input: text.into(),
        numeric: value.is_some(),
        value,
        ..SheetCell::default()
    };
    Sheet {
        name: "Datos 1".into(),
        row_count: 3,
        col_count: 2,
        col_widths: vec![64.0, 64.0],
        row_heights: vec![20.0, 20.0, 20.0],
        gridlines: true,
        cells: vec![
            text_cell(0, 1, "Caudal", None),
            text_cell(1, 0, "TR100", None),
            text_cell(1, 1, "99", Some(99.0)),
            text_cell(2, 0, "TR200", None),
            text_cell(2, 1, "120.5", Some(120.5)),
        ],
        ..Sheet::default()
    }
}

#[test]
fn reads_the_cells_when_a_chart_was_saved_without_its_cache() {
    // openpyxl writes references only; Excel fills the cache when it saves.
    let sheets = [datos_sheet()];
    let xml = format!(
        r#"<c:chartSpace {C}><c:chart><c:plotArea><c:lineChart><c:grouping val="standard"/>
      <c:ser><c:tx><c:strRef><c:f>'Datos 1'!$B$1</c:f></c:strRef></c:tx>
      <c:cat><c:strRef><c:f>'Datos 1'!$A$2:$A$3</c:f></c:strRef></c:cat>
      <c:val><c:numRef><c:f>'Datos 1'!$B$2:$B$3</c:f></c:numRef></c:val></c:ser>
      </c:lineChart></c:plotArea></c:chart></c:chartSpace>"#
    );
    let reader = RangeReader::new(&sheets);
    let chart = parse_chart(&xml, &theme(), &|f| reader.read(f));
    assert_eq!(chart.categories, ["TR100", "TR200"]);
    let series = &chart.series[0];
    assert_eq!(series.kind, ChartKind::Line);
    assert_eq!(series.name, "Caudal");
    assert_eq!(series.values, [Some(99.0), Some(120.5)]);
    assert_eq!(series.color.as_deref(), Some(theme()[4].as_str()));
}

#[test]
fn reads_ranges_across_sheets_unions_and_quoted_names() {
    let sheets = [datos_sheet()];
    let reader = RangeReader::new(&sheets);
    let range = reader.read("('Datos 1'!$B$2,'Datos 1'!$B$3)");
    assert_eq!(range.texts, ["99", "120.5"]);
    assert_eq!(range.numbers, [Some(99.0), Some(120.5)]);
    // An unknown sheet falls back to the first; empty cells read as nothing.
    let fallback = reader.read("Otra!A1:B1");
    assert_eq!(fallback.texts, ["", "Caudal"]);
    assert_eq!(fallback.numbers, [None, None]);
    assert_eq!(reader.read("not a reference"), RangeValues::default());
}

#[test]
fn gives_a_pie_its_point_colours_and_names_charts_it_does_not_draw() {
    let pie = parse_chart(
        &format!(
            r#"<c:chartSpace {C} {A}><c:chart><c:plotArea><c:pieChart><c:ser>
        <c:dPt><c:idx val="1"/><c:spPr><a:solidFill><a:schemeClr val="accent6"/></a:solidFill></c:spPr></c:dPt>
        <c:val><c:numLit><c:ptCount val="3"/><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt><c:pt idx="2"><c:v>3</c:v></c:pt></c:numLit></c:val>
      </c:ser></c:pieChart></c:plotArea></c:chart></c:chartSpace>"#
        ),
        &theme(),
        &no_ranges,
    );
    let t = theme();
    assert_eq!(
        pie.series[0].point_colors,
        Some(vec![
            Some(t[4].clone()),
            Some(t[9].clone()),
            Some(t[6].clone())
        ])
    );
    assert_eq!(pie.categories, ["1", "2", "3"]);

    let radar = parse_chart(
        &format!(
            r#"<c:chartSpace {C}><c:chart><c:plotArea><c:radarChart/></c:plotArea></c:chart></c:chartSpace>"#
        ),
        &theme(),
        &no_ranges,
    );
    assert_eq!(radar.unsupported.as_deref(), Some("radar"));
}

#[test]
fn reads_bars_stacking_scatter_and_a_single_series_title() {
    let chart = parse_chart(
        &format!(
            r#"<c:chartSpace {C}><c:chart><c:title/><c:plotArea>
        <c:barChart><c:barDir val="bar"/><c:grouping val="percentStacked"/>
          <c:ser><c:tx><c:v>Unica</c:v></c:tx><c:val><c:numLit><c:pt idx="0"><c:v>5</c:v></c:pt></c:numLit></c:val></c:ser>
        </c:barChart></c:plotArea></c:chart></c:chartSpace>"#
        ),
        &theme(),
        &no_ranges,
    );
    assert_eq!(chart.series[0].kind, ChartKind::Bar);
    assert_eq!(chart.stacking, Some(Stacking::Percent));
    // An empty title on a one-series chart is the series' name.
    assert_eq!(chart.title.as_deref(), Some("Unica"));
    assert!(!chart.legend);

    let scatter = parse_chart(
        &format!(
            r#"<c:chartSpace {C} {A}><c:chart><c:plotArea><c:scatterChart><c:ser>
          <c:spPr><a:ln><a:solidFill><a:srgbClr val="00FF00"/></a:solidFill></a:ln></c:spPr>
          <c:xVal><c:numLit><c:pt idx="0"><c:v>1</c:v></c:pt><c:pt idx="1"><c:v>2</c:v></c:pt></c:numLit></c:xVal>
          <c:yVal><c:numLit><c:pt idx="0"><c:v>10</c:v></c:pt><c:pt idx="1"><c:v>20</c:v></c:pt></c:numLit></c:yVal>
        </c:ser></c:scatterChart></c:plotArea></c:chart></c:chartSpace>"#
        ),
        &theme(),
        &no_ranges,
    );
    let series = &scatter.series[0];
    assert_eq!(series.kind, ChartKind::Scatter);
    assert_eq!(series.x_values, Some(vec![Some(1.0), Some(2.0)]));
    assert_eq!(series.values, [Some(10.0), Some(20.0)]);
    assert_eq!(series.color.as_deref(), Some("#00ff00"));
    assert_eq!(series.name, "Series 1");
}

fn drawing_workbook(anchor: &str, with_chart: bool) -> Vec<u8> {
    let workbook = format!(
        r#"<workbook xmlns="{MAIN}" xmlns:r="{REL}"><sheets><sheet name="Figuras" sheetId="1" r:id="rId1"/><sheet name="Vacía" sheetId="2" r:id="rId2"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="{REL}/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#
    );
    let sheet1 = format!(
        r#"<worksheet xmlns="{MAIN}" xmlns:r="{REL}"><cols><col min="1" max="1" width="20" customWidth="1"/></cols><sheetData><row r="1"><c r="A1" t="inlineStr"><is><t>Item</t></is></c><c r="B1" t="inlineStr"><is><t>Caudal</t></is></c></row><row r="2"><c r="A2" t="inlineStr"><is><t>TR100</t></is></c><c r="B2"><v>99</v></c></row></sheetData><drawing r:id="rId1"/></worksheet>"#
    );
    let sheet1_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/drawing" Target="../drawings/drawing1.xml"/></Relationships>"#
    );
    let sheet2 = format!(
        r#"<worksheet xmlns="{MAIN}"><sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData></worksheet>"#
    );
    let drawing = format!(r#"<xdr:wsDr {XDR} {A} xmlns:r="{REL}">{anchor}</xdr:wsDr>"#);
    let drawing_rels = format!(
        r#"<Relationships xmlns="{RELS_NS}"><Relationship Id="rId1" Type="{REL}/image" Target="../media/image1.png"/><Relationship Id="rId2" Type="{REL}/chart" Target="../charts/chart1.xml"/></Relationships>"#
    );
    let chart = format!(
        r#"<c:chartSpace {C}><c:chart><c:plotArea><c:barChart><c:barDir val="col"/><c:ser><c:tx><c:strRef><c:f>Figuras!$B$1</c:f></c:strRef></c:tx><c:cat><c:strRef><c:f>Figuras!$A$2</c:f></c:strRef></c:cat><c:val><c:numRef><c:f>Figuras!$B$2</c:f></c:numRef></c:val></c:ser></c:barChart></c:plotArea></c:chart></c:chartSpace>"#
    );
    let image = png();
    let mut entries: Vec<(&str, &[u8])> = vec![
        ("xl/workbook.xml", workbook.as_bytes()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.as_bytes()),
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
        ("xl/media/image1.png", image.as_slice()),
    ];
    if with_chart {
        entries.push(("xl/charts/chart1.xml", chart.as_bytes()));
    }
    package(&entries)
}

fn marker_xml(tag: &str, col: u32, row: u32) -> String {
    format!("<xdr:{tag}><xdr:col>{col}</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:{tag}>")
}

#[test]
fn places_a_picture_by_the_cells_it_is_anchored_to() {
    let anchor = format!(
        r#"<xdr:twoCellAnchor>{}{}<xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="Plano" descr="Planta"/><xdr:cNvPicPr/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic></xdr:twoCellAnchor>"#,
        marker_xml("from", 1, 2),
        marker_xml("to", 4, 8),
    );
    let Workbook { sheets } = parse_workbook(
        drawing_workbook(&anchor, false),
        "/obra/fig.xlsx",
        &Locale::en(),
    )
    .expect("a workbook");
    let [figuras, vacia] = &sheets[..] else {
        panic!("two sheets");
    };
    assert!(vacia.drawings.is_none());
    let drawings = figuras.drawings.as_ref().expect("drawings");
    assert_eq!(drawings.len(), 1);
    let SheetDrawing::Image(picture) = &drawings[0] else {
        panic!("a picture");
    };
    assert_eq!(picture.extension, "png");
    assert_eq!(picture.bytes, png());
    assert_eq!(picture.name, "Plano");
    assert_eq!(picture.description.as_deref(), Some("Planta"));
    // Column A is 20 characters wide (145 px); rows are 20 px.
    assert_eq!(column_pixels(20.0), 145.0);
    assert_eq!(picture.bounds.x, 145.0);
    assert_eq!(picture.bounds.y, 40.0);
    assert!((picture.bounds.width - 64.0 * 3.0).abs() < 1.0);
    assert!((picture.bounds.height - 20.0 * 6.0).abs() < 1.0);
}

#[test]
fn sizes_a_one_cell_anchor_and_grows_the_grid_to_hold_it() {
    let anchor = format!(
        r#"<xdr:oneCellAnchor>{}<xdr:ext cx="952500" cy="1905000"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="P"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic></xdr:oneCellAnchor>"#,
        marker_xml("from", 30, 100),
    );
    let Workbook { sheets } = parse_workbook(
        drawing_workbook(&anchor, false),
        "/obra/fig.xlsx",
        &Locale::en(),
    )
    .expect("a workbook");
    let figuras = &sheets[0];
    let SheetDrawing::Image(picture) = &figuras.drawings.as_ref().expect("drawings")[0] else {
        panic!("a picture");
    };
    assert_eq!(
        (picture.bounds.width, picture.bounds.height),
        (100.0, 200.0)
    );
    // The grid reaches the picture's far corner.
    assert!(figuras.row_count > 100 + 10);
    assert!(figuras.col_count > 30);
    assert_eq!(figuras.col_widths.len(), figuras.col_count);
}

#[test]
fn draws_a_chart_from_the_cells_it_names() {
    let anchor = format!(
        r#"<xdr:twoCellAnchor>{}{}<xdr:graphicFrame><xdr:nvGraphicFramePr><xdr:cNvPr id="3" name="Grafico"/></xdr:nvGraphicFramePr><a:graphic><a:graphicData><c:chart xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" r:id="rId2"/></a:graphicData></a:graphic></xdr:graphicFrame></xdr:twoCellAnchor>"#,
        marker_xml("from", 3, 0),
        marker_xml("to", 8, 10),
    );
    let Workbook { sheets } = parse_workbook(
        drawing_workbook(&anchor, true),
        "/obra/c.xlsx",
        &Locale::en(),
    )
    .expect("a workbook");
    let SheetDrawing::Chart(chart) = &sheets[0].drawings.as_ref().expect("drawings")[0] else {
        panic!("a chart");
    };
    assert_eq!(chart.categories, ["TR100"]);
    assert_eq!(chart.series[0].name, "Caudal");
    assert_eq!(chart.series[0].values, [Some(99.0)]);
    assert!(chart.bounds.width > 0.0 && chart.bounds.height > 0.0);
}

#[test]
fn skips_a_drawing_it_cannot_read_and_keeps_the_rest() {
    let anchor = format!(
        r#"<xdr:oneCellAnchor>{from}<xdr:ext cx="9525" cy="9525"/><xdr:pic><xdr:blipFill><a:blip r:embed="rId99"/></xdr:blipFill></xdr:pic></xdr:oneCellAnchor><xdr:oneCellAnchor>{from}<xdr:ext cx="9525" cy="9525"/><xdr:pic><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic></xdr:oneCellAnchor>"#,
        from = marker_xml("from", 0, 0),
    );
    let Workbook { sheets } = parse_workbook(
        drawing_workbook(&anchor, false),
        "/obra/c.xlsx",
        &Locale::en(),
    )
    .expect("a workbook");
    assert_eq!(sheets[0].drawings.as_ref().map(Vec::len), Some(1));
}

#[test]
fn serializes_drawings_with_a_kind_and_a_box() {
    let anchor = format!(
        r#"<xdr:oneCellAnchor>{}<xdr:ext cx="9525" cy="9525"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id="2" name="P"/></xdr:nvPicPr><xdr:blipFill><a:blip r:embed="rId1"/></xdr:blipFill></xdr:pic></xdr:oneCellAnchor>"#,
        marker_xml("from", 0, 0),
    );
    let Workbook { sheets } = parse_workbook(
        drawing_workbook(&anchor, false),
        "/obra/c.xlsx",
        &Locale::en(),
    )
    .expect("a workbook");
    let json = serde_json::to_value(&sheets[0]).expect("json");
    let drawing = &json["drawings"][0];
    assert_eq!(drawing["kind"], "image");
    assert_eq!(drawing["box"]["width"], 1.0);
    let round: Sheet = serde_json::from_value(json).expect("round trip");
    assert_eq!(round, sheets[0]);
}
