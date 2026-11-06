//! What a workbook draws over its cells: pictures and charts, read from the
//! file's drawing parts. Each lands on its sheet with a pixel box measured
//! from the cells it is anchored to. Ported from `spreadsheetDrawings.ts`.
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use super::xlsx::Sheet;
use super::xml::{number, parse_xml, resolve_part, Package, Rels, XmlNode};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct DrawingBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetImage {
    #[serde(rename = "box")]
    pub bounds: DrawingBox,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The picture's bytes, as stored; the viewer checks what they are.
    pub bytes: Vec<u8>,
    /// Its extension in the file, for a format the viewer cannot draw (EMF).
    pub extension: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChartKind {
    Column,
    Bar,
    Line,
    Area,
    Pie,
    Doughnut,
    Scatter,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeries {
    pub kind: ChartKind,
    pub name: String,
    pub values: Vec<Option<f64>>,
    /// A scatter series' x values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_values: Option<Vec<Option<f64>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// One colour per point, for a pie.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub point_colors: Option<Vec<Option<String>>>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Stacking {
    Stacked,
    Percent,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetChart {
    #[serde(rename = "box")]
    pub bounds: DrawingBox,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stacking: Option<Stacking>,
    pub legend: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y_title: Option<String>,
    /// A chart type this does not draw, by its XML name: shown as a card.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SheetDrawing {
    Image(SheetImage),
    Chart(SheetChart),
}

// ---------------------------------------------------------------- anchors

const EMU_PER_PIXEL: f64 = 9525.0;
const DEFAULT_COL: f64 = 64.0;
const DEFAULT_ROW: f64 = 20.0;
const MAX_COLS: usize = 16_384;
const MAX_ROWS: usize = 1_048_576;
/// A picture larger than this is left out.
const MAX_IMAGE_BYTES: usize = 25 * 1024 * 1024;
/// A chart reads at most this many points from a series.
const MAX_POINTS: usize = 100_000;

struct Marker {
    col: usize,
    col_off: f64,
    row: usize,
    row_off: f64,
}

fn marker(node: Option<&XmlNode>) -> Option<Marker> {
    let node = node?;
    let value = |name: &str| {
        number(node.child(name).map(|n| n.text.as_str()))
            .unwrap_or(0.0)
            .max(0.0)
    };
    Some(Marker {
        col: value("col") as usize,
        col_off: value("colOff") / EMU_PER_PIXEL,
        row: value("row") as usize,
        row_off: value("rowOff") / EMU_PER_PIXEL,
    })
}

fn extent(node: Option<&XmlNode>) -> Option<(f64, f64)> {
    let node = node?;
    Some((
        node.number("cx").unwrap_or(0.0) / EMU_PER_PIXEL,
        node.number("cy").unwrap_or(0.0) / EMU_PER_PIXEL,
    ))
}

/// Grow a sheet's grid until it reaches `col`/`row`, so a drawing has cells under it.
fn reach(sheet: &mut Sheet, col: usize, row: usize) {
    while sheet.col_widths.len() <= col {
        sheet.col_widths.push(DEFAULT_COL);
    }
    while sheet.row_heights.len() <= row {
        sheet.row_heights.push(DEFAULT_ROW);
    }
    sheet.col_count = sheet.col_widths.len();
    sheet.row_count = sheet.row_heights.len();
}

fn offset(sizes: &[f64], index: usize) -> f64 {
    (0..index)
        .map(|i| sizes.get(i).copied().unwrap_or(0.0))
        .sum()
}

#[derive(Clone, Copy)]
enum Axis {
    Columns,
    Rows,
}

/// The index whose span holds a pixel position, growing the grid to reach it.
fn index_for(sheet: &mut Sheet, axis: Axis, position: f64) -> usize {
    let mut total = 0.0;
    let mut index = 0;
    loop {
        let length = match axis {
            Axis::Columns => sheet.col_widths.len(),
            Axis::Rows => sheet.row_heights.len(),
        };
        if index >= length {
            match axis {
                Axis::Columns => reach(sheet, index, 0),
                Axis::Rows => reach(sheet, 0, index),
            }
        }
        total += match axis {
            Axis::Columns => sheet.col_widths[index],
            Axis::Rows => sheet.row_heights[index],
        };
        if total > position || index > 20_000 {
            return index;
        }
        index += 1;
    }
}

/// The pixel box of an anchor, against the sheet's own column widths and row heights.
pub fn anchor_box(anchor: &XmlNode, sheet: &mut Sheet) -> Option<DrawingBox> {
    if anchor.name == "absoluteAnchor" {
        let position = anchor.child("pos")?;
        let (width, height) = extent(anchor.child("ext"))?;
        let x = position.number("x").unwrap_or(0.0) / EMU_PER_PIXEL;
        let y = position.number("y").unwrap_or(0.0) / EMU_PER_PIXEL;
        index_for(sheet, Axis::Columns, x + width);
        index_for(sheet, Axis::Rows, y + height);
        return Some(DrawingBox {
            x,
            y,
            width,
            height,
        });
    }
    let from = marker(anchor.child("from"))?;
    if from.col > MAX_COLS || from.row > MAX_ROWS {
        return None;
    }
    reach(sheet, from.col + 1, from.row + 1);
    let x = offset(&sheet.col_widths, from.col) + from.col_off;
    let y = offset(&sheet.row_heights, from.row) + from.row_off;
    if anchor.name == "oneCellAnchor" {
        let (width, height) = extent(anchor.child("ext"))?;
        index_for(sheet, Axis::Columns, x + width);
        index_for(sheet, Axis::Rows, y + height);
        return Some(DrawingBox {
            x,
            y,
            width,
            height,
        });
    }
    let to = marker(anchor.child("to"))?;
    if to.col > MAX_COLS || to.row > MAX_ROWS {
        return None;
    }
    reach(sheet, to.col + 1, to.row + 1);
    let right = offset(&sheet.col_widths, to.col) + to.col_off;
    let bottom = offset(&sheet.row_heights, to.row) + to.row_off;
    Some(DrawingBox {
        x,
        y,
        width: (right - x).max(0.0),
        height: (bottom - y).max(0.0),
    })
}

// ---------------------------------------------------------------- ranges

/// A range's values, as a chart reads them from its sheet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RangeValues {
    pub texts: Vec<String>,
    pub numbers: Vec<Option<f64>>,
}

static REFERENCE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(?:('(?:[^']|'')+'|[^!]+)!)?\$?([A-Z]+)\$?(\d+)(?::\$?([A-Z]+)\$?(\d+))?$")
        .expect("a valid pattern")
});

fn decode_sheet_name(name: &str) -> String {
    let inner = name
        .strip_prefix('\'')
        .and_then(|n| n.strip_suffix('\''))
        .unwrap_or(name);
    inner.replace("''", "'")
}

fn column_index(letters: &str) -> usize {
    letters
        .to_ascii_uppercase()
        .bytes()
        .fold(0usize, |total, b| total * 26 + usize::from(b - 64))
        .saturating_sub(1)
}

/// Splits at commas outside single quotes.
fn split_references(formula: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut quoted = false;
    let mut start = 0;
    for (at, ch) in formula.char_indices() {
        if ch == '\'' {
            quoted = !quoted;
        } else if ch == ',' && !quoted {
            parts.push(&formula[start..at]);
            start = at + 1;
        }
    }
    parts.push(&formula[start..]);
    parts
}

/// Where each cell of a sheet sits in its cell list, by row and column.
type CellIndex = HashMap<(usize, usize), usize>;

/// Reads `'Hoja 1'!$B$2:$B$5` (or a union of such ranges) from the sheets,
/// row by row: the texts they print and the numbers behind them.
pub struct RangeReader<'a> {
    sheets: &'a [Sheet],
    indexes: RefCell<HashMap<usize, CellIndex>>,
}

impl<'a> RangeReader<'a> {
    pub fn new(sheets: &'a [Sheet]) -> RangeReader<'a> {
        RangeReader {
            sheets,
            indexes: RefCell::new(HashMap::new()),
        }
    }

    pub fn read(&self, formula: &str) -> RangeValues {
        let mut out = RangeValues::default();
        let formula = formula.strip_prefix('(').unwrap_or(formula);
        let formula = formula.strip_suffix(')').unwrap_or(formula);
        for part in split_references(formula) {
            let Some(captures) = REFERENCE.captures(part.trim()) else {
                continue;
            };
            let name = decode_sheet_name(captures.get(1).map_or("", |m| m.as_str()));
            let at = self.sheets.iter().position(|sheet| sheet.name == name).or(
                if self.sheets.is_empty() {
                    None
                } else {
                    Some(0)
                },
            );
            let Some(at) = at else {
                continue;
            };
            let mut indexes = self.indexes.borrow_mut();
            let index = indexes.entry(at).or_insert_with(|| {
                self.sheets[at]
                    .cells
                    .iter()
                    .enumerate()
                    .map(|(i, cell)| ((cell.row, cell.col), i))
                    .collect()
            });
            let (c1, r1) = (
                column_index(&captures[2]),
                captures[3].parse::<usize>().unwrap_or(1).saturating_sub(1),
            );
            let (c2, r2) = match (captures.get(4), captures.get(5)) {
                (Some(col), Some(row)) => (
                    column_index(col.as_str()),
                    row.as_str().parse::<usize>().unwrap_or(1).saturating_sub(1),
                ),
                _ => (c1, r1),
            };
            for row in r1.min(r2)..=r1.max(r2) {
                for col in c1.min(c2)..=c1.max(c2) {
                    if out.texts.len() >= MAX_POINTS {
                        return out;
                    }
                    let cell = index.get(&(row, col)).map(|i| &self.sheets[at].cells[*i]);
                    out.texts
                        .push(cell.map_or(String::new(), |c| c.text.clone()));
                    out.numbers.push(cell.and_then(|c| c.value));
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------- charts

fn rich_text(node: Option<&XmlNode>) -> Option<String> {
    let node = node?;
    let runs: Vec<&str> = node
        .find_all("t")
        .into_iter()
        .map(|t| t.text.as_str())
        .collect();
    if !runs.is_empty() {
        return Some(runs.concat());
    }
    let source = node.child("strRef").unwrap_or(node);
    let cached: Vec<&str> = source
        .find_all("v")
        .into_iter()
        .map(|v| v.text.as_str())
        .collect();
    (!cached.is_empty()).then(|| cached.join(" "))
}

fn title_of(node: Option<&XmlNode>, read: &dyn Fn(&str) -> RangeValues) -> Option<String> {
    let tx = node?.child("tx")?;
    let reference = tx.path(&["strRef", "f"]).map(|f| f.text.as_str());
    rich_text(Some(tx)).or_else(|| reference.map(|r| read(r).texts.join(" ")))
}

/// A data reference's points: its cache when saved, else the cells it names.
fn points(node: Option<&XmlNode>, read: &dyn Fn(&str) -> RangeValues) -> RangeValues {
    let Some(node) = node else {
        return RangeValues::default();
    };
    let reference = node
        .child("numRef")
        .or_else(|| node.child("strRef"))
        .or_else(|| node.child("multiLvlStrRef"));
    let literal = node.child("numLit").or_else(|| node.child("strLit"));
    let cache = reference
        .and_then(|r| {
            r.child("numCache")
                .or_else(|| r.child("strCache"))
                .or_else(|| r.child("multiLvlStrCache"))
        })
        .or(literal);
    let found: Vec<&XmlNode> = cache
        .map(|c| c.child("lvl").unwrap_or(c).find_all("pt"))
        .unwrap_or_default();
    if let (Some(cache), false) = (cache, found.is_empty()) {
        let indexed = |pt: &XmlNode| pt.number("idx").unwrap_or(0.0).max(0.0) as usize;
        let count = cache
            .child("ptCount")
            .and_then(|c| c.number("val"))
            .unwrap_or(0.0) as usize;
        let size = found
            .iter()
            .map(|pt| indexed(pt) + 1)
            .max()
            .unwrap_or(0)
            .max(count)
            .min(MAX_POINTS);
        let mut texts = vec![String::new(); size];
        let mut numbers = vec![None; size];
        for pt in found {
            let index = indexed(pt);
            if index >= size {
                continue;
            }
            let value = pt.child("v").map_or("", |v| v.text.as_str());
            texts[index] = value.to_string();
            numbers[index] = number(Some(value));
        }
        return RangeValues { texts, numbers };
    }
    match reference.and_then(|r| r.child("f")) {
        Some(formula) => read(&formula.text),
        None => RangeValues::default(),
    }
}

fn scheme_color(name: &str, theme: &[String]) -> Option<String> {
    let index = match name {
        "bg1" | "lt1" => 0,
        "tx1" | "dk1" => 1,
        "bg2" | "lt2" => 2,
        "tx2" | "dk2" => 3,
        "accent1" => 4,
        "accent2" => 5,
        "accent3" => 6,
        "accent4" => 7,
        "accent5" => 8,
        "accent6" => 9,
        _ => return None,
    };
    theme.get(index).cloned()
}

fn fill_color(sp_pr: Option<&XmlNode>, theme: &[String], line: bool) -> Option<String> {
    let holder = if line { sp_pr?.child("ln") } else { sp_pr };
    let solid = holder?.child("solidFill")?;
    if let Some(rgb) = solid.child("srgbClr").and_then(|c| c.attr("val")) {
        return Some(format!("#{}", rgb.to_lowercase()));
    }
    scheme_color(solid.child("schemeClr")?.attr("val")?, theme)
}

fn group_kind(name: &str) -> Option<ChartKind> {
    Some(match name {
        "barChart" | "bar3DChart" => ChartKind::Column,
        "lineChart" | "line3DChart" => ChartKind::Line,
        "areaChart" | "area3DChart" => ChartKind::Area,
        "pieChart" | "pie3DChart" | "ofPieChart" => ChartKind::Pie,
        "doughnutChart" => ChartKind::Doughnut,
        "scatterChart" => ChartKind::Scatter,
        _ => return None,
    })
}

/// A chart part's drawing: its type, series, labels and colours. The box is
/// left empty for the caller to place.
pub fn parse_chart(xml: &str, theme: &[String], read: &dyn Fn(&str) -> RangeValues) -> SheetChart {
    let space = parse_xml(xml);
    let chart = space.find("chart");
    let plot = chart.and_then(|c| c.child("plotArea"));
    let auto_deleted = chart
        .and_then(|c| c.child("autoTitleDeleted"))
        .and_then(|n| n.attr("val"))
        == Some("1");
    let mut series: Vec<ChartSeries> = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    let mut stacking = None;
    let mut unsupported: Option<String> = None;
    let accents: Vec<&String> = theme.iter().skip(4).take(6).collect();
    let accent = |index: usize| {
        accents
            .get(index % accents.len().max(1))
            .map(|c| (*c).clone())
    };
    for group in plot.map(|p| p.children.as_slice()).unwrap_or_default() {
        if !group.name.ends_with("Chart") {
            continue;
        }
        let Some(mut kind) = group_kind(&group.name) else {
            unsupported.get_or_insert_with(|| group.name.trim_end_matches("Chart").to_string());
            continue;
        };
        if kind == ChartKind::Column
            && group.child("barDir").and_then(|n| n.attr("val")) == Some("bar")
        {
            kind = ChartKind::Bar;
        }
        match group.child("grouping").and_then(|n| n.attr("val")) {
            Some("stacked") => stacking = Some(Stacking::Stacked),
            Some("percentStacked") => stacking = Some(Stacking::Percent),
            _ => {}
        }
        for ser in group.children_named("ser") {
            let index = series.len();
            let name = title_of(Some(ser), read).unwrap_or_else(|| format!("Series {}", index + 1));
            let fallback = accent(index);
            let sp_pr = ser.child("spPr");
            let color = if matches!(kind, ChartKind::Line | ChartKind::Scatter) {
                fill_color(sp_pr, theme, true)
                    .or_else(|| fill_color(sp_pr, theme, false))
                    .or(fallback)
            } else {
                fill_color(sp_pr, theme, false).or(fallback)
            };
            if kind == ChartKind::Scatter {
                let x = points(ser.child("xVal"), read);
                let y = points(ser.child("yVal"), read);
                series.push(ChartSeries {
                    kind,
                    name,
                    values: y.numbers,
                    x_values: Some(x.numbers),
                    color,
                    point_colors: None,
                });
                continue;
            }
            let cat = points(ser.child("cat"), read);
            if categories.is_empty() && !cat.texts.is_empty() {
                categories = cat.texts;
            }
            let values = points(ser.child("val"), read).numbers;
            let mut entry = ChartSeries {
                kind,
                name,
                values,
                x_values: None,
                color,
                point_colors: None,
            };
            if matches!(kind, ChartKind::Pie | ChartKind::Doughnut) {
                let custom: HashMap<usize, Option<String>> = ser
                    .children_named("dPt")
                    .filter_map(|d_pt| {
                        let idx = d_pt.child("idx")?.number("val")?;
                        Some((idx as usize, fill_color(d_pt.child("spPr"), theme, false)))
                    })
                    .collect();
                entry.point_colors = Some(
                    (0..entry.values.len())
                        .map(|point| {
                            custom
                                .get(&point)
                                .cloned()
                                .flatten()
                                .or_else(|| accent(point))
                        })
                        .collect(),
                );
            }
            series.push(entry);
        }
    }
    if categories.is_empty() {
        let longest = series.iter().map(|s| s.values.len()).max().unwrap_or(0);
        categories = (1..=longest).map(|n| n.to_string()).collect();
    }
    // Each axis says where it sits: along the bottom it is x, up the side y.
    let (mut x_title, mut y_title): (Option<String>, Option<String>) = (None, None);
    for axis in plot.map(|p| p.children.as_slice()).unwrap_or_default() {
        if !matches!(axis.name.as_str(), "catAx" | "valAx" | "dateAx" | "serAx") {
            continue;
        }
        let Some(title) = title_of(axis.child("title"), read) else {
            continue;
        };
        match axis.child("axPos").and_then(|n| n.attr("val")) {
            Some("b" | "t") => {
                x_title.get_or_insert(title);
            }
            _ => {
                y_title.get_or_insert(title);
            }
        }
    }
    let chart_title = chart.and_then(|c| c.child("title"));
    let title = title_of(chart_title, read).or_else(|| {
        (chart_title.is_some() && !auto_deleted && series.len() == 1)
            .then(|| series[0].name.clone())
    });
    SheetChart {
        bounds: DrawingBox::default(),
        title,
        categories,
        legend: chart.is_some_and(|c| c.child("legend").is_some()),
        x_title,
        y_title,
        unsupported: if series.is_empty() { unsupported } else { None },
        stacking,
        series,
    }
}

// ---------------------------------------------------------------- a workbook's drawings

enum Found {
    Image {
        name: String,
        description: Option<String>,
        bytes: Vec<u8>,
        extension: String,
    },
    Chart(SheetChart),
}

struct Pending {
    sheet: usize,
    anchor: XmlNode,
    found: Found,
}

fn read_anchor(
    anchor: &XmlNode,
    drawing_part: &str,
    rels: &Rels,
    package: &mut Package,
    theme: &[String],
    read: &dyn Fn(&str) -> RangeValues,
) -> Option<Found> {
    if let Some(pic) = anchor.find("pic") {
        let embed = pic.find("blip")?.rel_attr("embed")?;
        let rel = rels.get(embed).filter(|rel| !rel.external)?;
        let part = resolve_part(drawing_part, &rel.target);
        let bytes = package
            .bytes(&part)
            .filter(|b| b.len() <= MAX_IMAGE_BYTES)?;
        let props = pic.find("cNvPr");
        return Some(Found::Image {
            name: props
                .and_then(|p| p.attr("name"))
                .map(str::to_string)
                .unwrap_or_else(|| part.rsplit('/').next().unwrap_or("Picture").to_string()),
            description: props
                .and_then(|p| p.attr("descr"))
                .filter(|d| !d.is_empty())
                .map(str::to_string),
            extension: part.rsplit('.').next().unwrap_or("").to_lowercase(),
            bytes,
        });
    }
    if let Some(frame) = anchor.find("graphicFrame") {
        let id = frame.find("chart")?.rel_attr("id")?;
        let rel = rels.get(id).filter(|rel| !rel.external)?;
        let xml = package.text(&resolve_part(drawing_part, &rel.target))?;
        return Some(Found::Chart(parse_chart(&xml, theme, read)));
    }
    None
}

/// Every sheet's pictures and charts. A part that cannot be read is skipped:
/// the cells still show. `parts[i]` is the package path of `sheets[i]`.
pub fn read_drawings(
    package: &mut Package,
    sheets: &mut [Sheet],
    parts: &[String],
    theme: &[String],
) {
    let mut pending: Vec<Pending> = Vec::new();
    {
        let reader = RangeReader::new(sheets);
        let read = |formula: &str| reader.read(formula);
        for (index, part) in parts.iter().enumerate().take(sheets.len()) {
            if part.is_empty() {
                continue;
            }
            let sheet_rels = package.rels(part);
            for rel in sheet_rels.iter() {
                if !rel.kind.ends_with("/drawing") || rel.external {
                    continue;
                }
                let drawing_part = resolve_part(part, &rel.target);
                let Some(drawing_xml) = package.text(&drawing_part) else {
                    continue;
                };
                let rels = package.rels(&drawing_part);
                let root = parse_xml(&drawing_xml);
                let anchors = root
                    .find("wsDr")
                    .map(|n| n.children.clone())
                    .unwrap_or_else(|| root.children.clone());
                for anchor in anchors {
                    if !matches!(
                        anchor.name.as_str(),
                        "twoCellAnchor" | "oneCellAnchor" | "absoluteAnchor"
                    ) {
                        continue;
                    }
                    // One unreadable picture or chart leaves the rest.
                    if let Some(found) =
                        read_anchor(&anchor, &drawing_part, &rels, package, theme, &read)
                    {
                        pending.push(Pending {
                            sheet: index,
                            anchor,
                            found,
                        });
                    }
                }
            }
        }
    }
    let mut placed: HashMap<usize, Vec<SheetDrawing>> = HashMap::new();
    for item in pending {
        let Some(bounds) = anchor_box(&item.anchor, &mut sheets[item.sheet]) else {
            continue;
        };
        let drawing = match item.found {
            Found::Image {
                name,
                description,
                bytes,
                extension,
            } => SheetDrawing::Image(SheetImage {
                bounds,
                name,
                description,
                bytes,
                extension,
            }),
            Found::Chart(chart) => SheetDrawing::Chart(SheetChart { bounds, ..chart }),
        };
        placed.entry(item.sheet).or_default().push(drawing);
    }
    for (index, drawings) in placed {
        sheets[index].drawings = Some(drawings);
    }
}

#[cfg(test)]
#[path = "drawings_tests.rs"]
mod tests;
