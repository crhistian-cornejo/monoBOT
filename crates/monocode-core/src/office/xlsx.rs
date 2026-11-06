//! A spreadsheet read for showing, not editing: each sheet's cells with the
//! text Excel would print, the input the formula bar shows, and the look the
//! file gave them (fills, fonts, borders, merges, sizes, frozen panes).
//!
//! Ported from the Tauri viewer's `spreadsheet.ts`. The workbook's XML parts
//! are read directly (the viewer used ExcelJS), so cell styles, merges and
//! the drawings over the cells all come from one pass over the package.
//! `.csv` and `.tsv` are split here too; the old binary `.xls` is refused
//! with a message that sends the user to Excel.
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use regex::Regex;
use serde::{Deserialize, Serialize};

use super::drawings::{self, SheetDrawing};
use super::numfmt::{
    format_date, format_number, is_date_format, js_number_string, DateTime, Locale,
};
use super::xml::{decode_entities, decode_reference, resolve_part, Package, XmlNode};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    Solid,
    Dashed,
    Dotted,
    Double,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CellBorder {
    pub width: u32,
    pub style: BorderStyle,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HAlign {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct CellStyle {
    /// `#rrggbb`. Absent: the theme's background.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    /// `#rrggbb`. Absent: automatic, readable on the fill or the theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Font size in points, when it is not Excel's 11.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// The font, when it is not Calibri.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<HAlign>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valign: Option<VAlign>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub wrap: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bottom: Option<CellBorder>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<CellBorder>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetCell {
    /// Zero-based.
    pub row: usize,
    pub col: usize,
    /// What the cell prints.
    pub text: String,
    /// What the formula bar shows: the formula, or the raw value.
    pub input: String,
    /// Numbers and dates lean right, as in Excel, unless styled otherwise.
    #[serde(default, skip_serializing_if = "is_false")]
    pub numeric: bool,
    /// The number behind a numeric cell, for the charts that read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// A formula saved without its result: the text is the formula itself.
    #[serde(default, skip_serializing_if = "is_false")]
    pub uncalculated: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub row_span: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub col_span: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<CellStyle>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    pub row_count: usize,
    pub col_count: usize,
    /// Pixels; 0 for a hidden column or row.
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<SheetCell>,
    pub frozen_rows: usize,
    pub frozen_cols: usize,
    pub gridlines: bool,
    /// The sheet had more cells than the viewer reads.
    pub truncated: bool,
    /// Pictures and charts over the cells, in the order the file draws them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drawings: Option<Vec<SheetDrawing>>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Workbook {
    pub sheets: Vec<Sheet>,
}

/// Past this many cells a sheet is cut short; the rest is in Excel.
pub const MAX_CELLS: usize = 250_000;

const DEFAULT_COL_WIDTH: f64 = 8.43;
const DEFAULT_ROW_HEIGHT: f64 = 15.0;
/// Empty rows and columns past the data, so the grid reads as a sheet.
const PAD_ROWS: usize = 20;
const PAD_COLS: usize = 4;
const MIN_ROWS: usize = 40;
const MIN_COLS: usize = 12;

const OLD_XLS: &str = "This is the old binary Excel format (.xls). Open it in Excel, or save it as .xlsx to read it here.";

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn extension_of(path: &str) -> String {
    let name = basename(path).to_lowercase();
    match name.rfind('.') {
        Some(dot) => name[dot..].to_string(),
        None => String::new(),
    }
}

/// Whether a path opens in the spreadsheet viewer rather than the editor.
pub fn is_spreadsheet_path(path: &str) -> bool {
    [".xlsx", ".xlsm", ".xls", ".csv", ".tsv"].contains(&extension_of(path).as_str())
}

/// An Excel workbook: binary, so the editor has nothing to show. CSV stays
/// text there, where it can be edited; the agents' panel shows it as a sheet.
pub fn is_workbook_path(path: &str) -> bool {
    [".xlsx", ".xlsm", ".xls"].contains(&extension_of(path).as_str())
}

/// The legacy binary format, which only Excel itself opens.
pub fn is_legacy_xls(path: &str) -> bool {
    extension_of(path) == ".xls"
}

/// 0 is A, 25 is Z, 26 is AA.
pub fn column_name(index: usize) -> String {
    let mut name = Vec::new();
    let mut n = index + 1;
    while n > 0 {
        name.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

pub fn cell_address(row: usize, col: usize) -> String {
    format!("{}{}", column_name(col), row + 1)
}

static ADDRESS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\$?([A-Z]+)\$?(\d+)$").expect("a valid pattern"));

/// `B12` is zero-based row 11, column 1.
pub fn parse_address(address: &str) -> Option<(usize, usize)> {
    let captures = ADDRESS.captures(address.trim())?;
    let col = captures[1]
        .to_ascii_uppercase()
        .bytes()
        .fold(0usize, |total, b| total * 26 + usize::from(b - 64));
    let row: usize = captures[2].parse().ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}

/// Excel's character widths in screen pixels.
pub fn column_pixels(width: f64) -> f64 {
    (width * 7.0 + 5.0).round()
}

/// Excel's point heights in screen pixels.
pub fn row_pixels(points: f64) -> f64 {
    (points * 4.0 / 3.0).round()
}

// ---------------------------------------------------------------- colours

/// Office's default palette, in the order a theme index counts them.
const OFFICE_THEME: [&str; 12] = [
    "#ffffff", "#000000", "#e7e6e6", "#44546a", "#4472c4", "#ed7d31", "#a5a5a5", "#ffc000",
    "#5b9bd5", "#70ad47", "#0563c1", "#954f72",
];

/// The workbook's theme colours by theme index. The XML lists dark before
/// light (dk1, lt1, dk2, lt2...) while an index counts light first, so the
/// first two pairs swap. `xml` is the parsed theme part, or nothing.
pub fn theme_colors(xml: Option<&XmlNode>) -> Vec<String> {
    let office = || OFFICE_THEME.iter().map(|c| c.to_string()).collect();
    let Some(xml) = xml else {
        return office();
    };
    let names = [
        "dk1", "lt1", "dk2", "lt2", "accent1", "accent2", "accent3", "accent4", "accent5",
        "accent6", "hlink", "folHlink",
    ];
    let hex = |value: &str| -> Option<String> {
        (value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| format!("#{}", value.to_lowercase()))
    };
    let found: Vec<Option<String>> = names
        .iter()
        .map(|name| {
            let block = xml.find(name)?;
            let last = block
                .find_all("sysClr")
                .into_iter()
                .find_map(|node| node.attr("lastClr").and_then(hex));
            last.or_else(|| {
                block
                    .find("srgbClr")
                    .and_then(|node| node.attr("val").and_then(hex))
            })
        })
        .collect();
    let order = [1, 0, 3, 2, 4, 5, 6, 7, 8, 9, 10, 11];
    order
        .iter()
        .enumerate()
        .map(|(index, source)| {
            found[*source]
                .clone()
                .unwrap_or_else(|| OFFICE_THEME[index].to_string())
        })
        .collect()
}

fn to_rgb(hex: &str) -> [f64; 3] {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    [
        f64::from((value >> 16) & 255),
        f64::from((value >> 8) & 255),
        f64::from(value & 255),
    ]
}

fn to_hex(rgb: [f64; 3]) -> String {
    let part = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    format!(
        "#{:02x}{:02x}{:02x}",
        part(rgb[0]),
        part(rgb[1]),
        part(rgb[2])
    )
}

/// Excel's tint: toward white when positive, toward black when negative.
pub fn apply_tint(hex: &str, tint: f64) -> String {
    if tint == 0.0 {
        return hex.to_string();
    }
    to_hex(to_rgb(hex).map(|part| {
        if tint < 0.0 {
            part * (1.0 + tint)
        } else {
            part + (255.0 - part) * tint
        }
    }))
}

/// A colour as the styles part writes it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct XColor {
    pub argb: Option<String>,
    pub theme: Option<usize>,
    pub tint: Option<f64>,
    pub indexed: Option<usize>,
}

impl XColor {
    fn from_node(node: Option<&XmlNode>) -> Option<XColor> {
        let node = node?;
        let argb = node.attr("rgb").map(|rgb| {
            if rgb.len() == 6 {
                format!("FF{rgb}")
            } else {
                rgb.to_string()
            }
        });
        Some(XColor {
            argb,
            theme: node.number("theme").map(|n| n as usize),
            tint: node.number("tint"),
            indexed: node.number("indexed").map(|n| n as usize),
        })
    }
}

/// The legacy palette's first entries, the only ones files still use often.
const INDEXED: [&str; 8] = [
    "#000000", "#ffffff", "#ff0000", "#00ff00", "#0000ff", "#ffff00", "#ff00ff", "#00ffff",
];

pub fn excel_color(color: Option<&XColor>, theme: &[String]) -> Option<String> {
    let color = color?;
    if let Some(argb) = &color.argb {
        if argb.len() == 8 && argb.chars().all(|c| c.is_ascii_hexdigit()) {
            // A fully transparent colour is no colour.
            if argb.starts_with("00") && argb != "00000000" {
                return None;
            }
            return Some(apply_tint(
                &format!("#{}", argb[2..].to_lowercase()),
                color.tint.unwrap_or(0.0),
            ));
        }
    }
    if let Some(index) = color.theme {
        if let Some(base) = theme.get(index) {
            return Some(apply_tint(base, color.tint.unwrap_or(0.0)));
        }
    }
    if let Some(indexed) = color.indexed {
        let index = if (8..16).contains(&indexed) {
            indexed - 8
        } else {
            indexed
        };
        return INDEXED.get(index).map(|c| c.to_string());
    }
    None
}

/// Black or white, whichever reads on the fill.
pub fn readable_on(fill: &str) -> &'static str {
    let [r, g, b] = to_rgb(fill);
    if 0.299 * r + 0.587 * g + 0.114 * b > 150.0 {
        "#000000"
    } else {
        "#ffffff"
    }
}

/// Black or white set on purpose reads as Excel's "automatic".
fn automatic(color: Option<&XColor>) -> bool {
    let Some(color) = color else {
        return true;
    };
    if matches!(color.theme, Some(0 | 1)) {
        return color.tint.unwrap_or(0.0) == 0.0;
    }
    if let Some(argb) = &color.argb {
        let lower = argb.to_lowercase();
        return lower.len() == 8 && (lower.ends_with("000000") || lower.ends_with("ffffff"));
    }
    matches!(color.indexed, Some(64 | 8 | 9))
}

// ---------------------------------------------------------------- styles

/// The built-in number formats files refer to by id without defining them.
fn builtin_format(id: u32) -> Option<&'static str> {
    Some(match id {
        1 => "0",
        2 => "0.00",
        3 => "#,##0",
        4 => "#,##0.00",
        9 => "0%",
        10 => "0.00%",
        11 => "0.00E+00",
        12 => "# ?/?",
        13 => "# ??/??",
        14 => "mm-dd-yy",
        15 => "d-mmm-yy",
        16 => "d-mmm",
        17 => "mmm-yy",
        18 => "h:mm AM/PM",
        19 => "h:mm:ss AM/PM",
        20 => "h:mm",
        21 => "h:mm:ss",
        22 => "m/d/yy h:mm",
        37 => "#,##0 ;(#,##0)",
        38 => "#,##0 ;[Red](#,##0)",
        39 => "#,##0.00;(#,##0.00)",
        40 => "#,##0.00;[Red](#,##0.00)",
        45 => "mm:ss",
        46 => "[h]:mm:ss",
        47 => "mmss.0",
        48 => "##0.0E+0",
        49 => "@",
        _ => return None,
    })
}

/// A cell format's look and number format, resolved once per `cellXfs` entry.
#[derive(Debug, Clone, Default)]
struct Xf {
    num_fmt: Option<String>,
    style: Option<CellStyle>,
}

fn flag(node: Option<&XmlNode>) -> bool {
    node.is_some_and(|n| !matches!(n.attr("val"), Some("0" | "false")))
}

fn border_side(node: Option<&XmlNode>, theme: &[String]) -> Option<CellBorder> {
    let node = node?;
    let style = node.attr("style")?;
    let width = if style.contains("medium") || style.contains("thick") {
        if style == "thick" {
            3
        } else {
            2
        }
    } else {
        1
    };
    let lower = style.to_lowercase();
    let kind = if style == "double" {
        BorderStyle::Double
    } else if lower.contains("dash") {
        BorderStyle::Dashed
    } else if lower.contains("dot") || lower.contains("hair") {
        BorderStyle::Dotted
    } else {
        BorderStyle::Solid
    };
    let color = XColor::from_node(node.child("color"));
    Some(CellBorder {
        width: if kind == BorderStyle::Double {
            3
        } else {
            width
        },
        style: kind,
        color: if automatic(color.as_ref()) {
            None
        } else {
            excel_color(color.as_ref(), theme)
        },
    })
}

fn cell_style(
    font: Option<&XmlNode>,
    fill: Option<&XmlNode>,
    border: Option<&XmlNode>,
    alignment: Option<&XmlNode>,
    theme: &[String],
) -> Option<CellStyle> {
    let mut out = CellStyle::default();
    if let Some(fill) = fill {
        if let Some(pattern) = fill.child("patternFill") {
            if !matches!(pattern.attr("patternType"), None | Some("none")) {
                out.fill = excel_color(XColor::from_node(pattern.child("fgColor")).as_ref(), theme)
                    .or_else(|| {
                        excel_color(XColor::from_node(pattern.child("bgColor")).as_ref(), theme)
                    });
            }
        } else if let Some(gradient) = fill.child("gradientFill") {
            let first = gradient.find("stop").and_then(|stop| stop.child("color"));
            out.fill = excel_color(XColor::from_node(first).as_ref(), theme);
        }
    }
    if let Some(font) = font {
        out.bold = flag(font.child("b"));
        out.italic = flag(font.child("i"));
        out.underline = font
            .child("u")
            .is_some_and(|u| !matches!(u.attr("val"), Some("none" | "0" | "false")));
        out.strike = flag(font.child("strike"));
        if let Some(size) = font.child("sz").and_then(|n| n.number("val")) {
            if size != 11.0 && size != 0.0 {
                out.size = Some(size);
            }
        }
        if let Some(name) = font.child("name").and_then(|n| n.attr("val")) {
            if name != "Calibri" && !name.is_empty() {
                out.font = Some(name.to_string());
            }
        }
        let color = XColor::from_node(font.child("color"));
        if !automatic(color.as_ref()) {
            out.color = excel_color(color.as_ref(), theme);
        }
    }
    if let Some(alignment) = alignment {
        out.align = match alignment.attr("horizontal") {
            Some("center" | "centerContinuous") => Some(HAlign::Center),
            Some("right") => Some(HAlign::Right),
            Some("left") => Some(HAlign::Left),
            Some("justify" | "distributed") => Some(HAlign::Justify),
            _ => None,
        };
        out.valign = match alignment.attr("vertical") {
            Some("top") => Some(VAlign::Top),
            Some("center") => Some(VAlign::Middle),
            _ => None,
        };
        out.wrap = matches!(alignment.attr("wrapText"), Some("1" | "true"));
        out.indent = alignment
            .number("indent")
            .filter(|n| *n > 0.0)
            .map(|n| n as u32);
    }
    if let Some(border) = border {
        out.top = border_side(border.child("top"), theme);
        out.right = border_side(border.child("right"), theme);
        out.bottom = border_side(border.child("bottom"), theme);
        out.left = border_side(border.child("left"), theme);
    }
    (out != CellStyle::default()).then_some(out)
}

fn read_styles(styles: Option<&XmlNode>, theme: &[String]) -> Vec<Xf> {
    let Some(root) = styles.and_then(|root| root.find("styleSheet").or(Some(root))) else {
        return Vec::new();
    };
    let custom: HashMap<u32, String> = root
        .child("numFmts")
        .map(|formats| {
            formats
                .children_named("numFmt")
                .filter_map(|fmt| {
                    Some((
                        fmt.number("numFmtId")? as u32,
                        fmt.attr("formatCode")?.to_string(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    fn list<'a>(root: &'a XmlNode, name: &str, item: &'a str) -> Vec<&'a XmlNode> {
        root.child(name)
            .map(|parent| parent.children_named(item).collect())
            .unwrap_or_default()
    }
    let fonts = list(root, "fonts", "font");
    let fills = list(root, "fills", "fill");
    let borders = list(root, "borders", "border");
    list(root, "cellXfs", "xf")
        .into_iter()
        .map(|xf| {
            let id = xf.number("numFmtId").unwrap_or(0.0) as u32;
            let num_fmt = custom
                .get(&id)
                .cloned()
                .or_else(|| builtin_format(id).map(str::to_string));
            let pick = |list: &[&XmlNode], key: &str| -> Option<usize> {
                xf.number(key)
                    .map(|n| n as usize)
                    .filter(|n| *n < list.len())
            };
            Xf {
                num_fmt,
                style: cell_style(
                    pick(&fonts, "fontId").map(|i| fonts[i]),
                    pick(&fills, "fillId").map(|i| fills[i]),
                    pick(&borders, "borderId").map(|i| borders[i]),
                    xf.child("alignment"),
                    theme,
                ),
            }
        })
        .collect()
}

// ---------------------------------------------------------------- values

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Number(f64),
    Text(String),
    Bool(bool),
    Date(DateTime),
    Error(String),
}

struct Printed {
    text: String,
    numeric: bool,
    error: bool,
}

/// A value and how it prints, given the cell's number format.
fn printed(value: &Value, format: Option<&str>, locale: &Locale) -> Printed {
    let plain = |text: String| Printed {
        text,
        numeric: false,
        error: false,
    };
    match value {
        Value::Number(number) => Printed {
            text: format_number(*number, format, locale),
            numeric: true,
            error: false,
        },
        Value::Bool(flag) => plain(if *flag { "TRUE" } else { "FALSE" }.into()),
        Value::Date(date) => Printed {
            text: format_date(date, format, locale),
            numeric: true,
            error: false,
        },
        Value::Text(text) => plain(text.clone()),
        Value::Error(code) => Printed {
            text: code.clone(),
            numeric: false,
            error: true,
        },
    }
}

fn input_of(value: &Value) -> String {
    match value {
        Value::Number(number) => js_number_string(*number),
        Value::Date(date) => date.input(),
        Value::Bool(flag) => if *flag { "TRUE" } else { "FALSE" }.into(),
        Value::Text(text) => text.clone(),
        Value::Error(code) => code.clone(),
    }
}

/// The text of a shared string item: plain, or its rich-text runs joined.
fn shared_text(item: &XmlNode) -> String {
    let mut out = String::new();
    for entry in &item.children {
        match entry.name.as_str() {
            "t" => out.push_str(&entry.text),
            "r" => {
                for text in entry.children_named("t") {
                    out.push_str(&text.text);
                }
            }
            _ => {}
        }
    }
    out
}

fn read_shared_strings(root: Option<&XmlNode>) -> Vec<String> {
    root.and_then(|root| root.find("sst").or(Some(root)))
        .map(|sst| sst.children_named("si").map(shared_text).collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------- shared formulas

/// A shared formula's text for a cell `d_row`/`d_col` away from the master,
/// shifting its relative references the way Excel fills them.
fn shift_formula(formula: &str, d_row: i64, d_col: i64) -> String {
    let chars: Vec<char> = formula.chars().collect();
    let mut out = String::with_capacity(formula.len());
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if ch == '"' || ch == '\'' {
            // A string or a quoted sheet name: copied as is.
            let end = chars[index + 1..]
                .iter()
                .position(|c| *c == ch)
                .map_or(chars.len() - 1, |at| at + index + 1);
            out.extend(&chars[index..=end]);
            index = end + 1;
            continue;
        }
        let previous = out.chars().last();
        let boundary = previous.is_none_or(|p| !(p.is_alphanumeric() || p == '_' || p == '.'));
        if boundary && (ch == '$' || ch.is_ascii_alphabetic()) {
            if let Some((reference, used)) = shift_reference(&chars[index..], d_row, d_col) {
                out.push_str(&reference);
                index += used;
                continue;
            }
        }
        out.push(ch);
        index += 1;
    }
    out
}

/// One `$A$1`-style reference at the start of `chars`, shifted, and the
/// characters it used.
fn shift_reference(chars: &[char], d_row: i64, d_col: i64) -> Option<(String, usize)> {
    let mut at = 0;
    let col_absolute = chars.get(at) == Some(&'$');
    if col_absolute {
        at += 1;
    }
    let letters_start = at;
    while chars.get(at).is_some_and(char::is_ascii_alphabetic) {
        at += 1;
    }
    let letters = at - letters_start;
    if !(1..=3).contains(&letters) {
        return None;
    }
    let row_absolute = chars.get(at) == Some(&'$');
    if row_absolute {
        at += 1;
    }
    let digits_start = at;
    while chars.get(at).is_some_and(char::is_ascii_digit) {
        at += 1;
    }
    if at == digits_start {
        return None;
    }
    // A function name or a longer word is not a reference.
    if chars
        .get(at)
        .is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '(' || *c == '.')
    {
        return None;
    }
    let letter_text: String = chars[letters_start..letters_start + letters]
        .iter()
        .collect();
    let col = letter_text
        .to_ascii_uppercase()
        .bytes()
        .fold(0i64, |total, b| total * 26 + i64::from(b - 64));
    let row: i64 = chars[digits_start..at]
        .iter()
        .collect::<String>()
        .parse()
        .ok()?;
    let new_col = if col_absolute { col } else { col + d_col };
    let new_row = if row_absolute { row } else { row + d_row };
    if !(1..=16384).contains(&new_col) || !(1..=1_048_576).contains(&new_row) {
        return Some(("#REF!".to_string(), at));
    }
    Some((
        format!(
            "{}{}{}{}",
            if col_absolute { "$" } else { "" },
            column_name((new_col - 1) as usize),
            if row_absolute { "$" } else { "" },
            new_row
        ),
        at,
    ))
}

// ---------------------------------------------------------------- sheet parts

/// A `<c>` as the sheet part wrote it.
#[derive(Default)]
struct RawCell {
    address: String,
    kind: String,
    style: usize,
    value: Option<String>,
    inline: Option<String>,
    formula: Option<String>,
    shared_index: Option<String>,
}

fn local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn start_attr(start: &BytesStart<'_>, key: &str) -> Option<String> {
    for attribute in start.attributes().with_checks(false).flatten() {
        if local(attribute.key.as_ref()) == key {
            return Some(decode_entities(&attribute.value));
        }
    }
    None
}

fn start_number(start: &BytesStart<'_>, key: &str) -> Option<f64> {
    start_attr(start, key)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|n| n.is_finite())
}

fn start_flag(start: &BytesStart<'_>, key: &str) -> bool {
    matches!(start_attr(start, key).as_deref(), Some("1" | "true"))
}

#[derive(Default)]
struct ColumnDef {
    min: usize,
    max: usize,
    width: Option<f64>,
    hidden: bool,
}

/// What reading a sheet part leaves, before the grid is laid out.
#[derive(Default)]
struct SheetParts {
    cells: Vec<SheetCell>,
    row_heights: HashMap<usize, f64>,
    columns: Vec<ColumnDef>,
    merges: Vec<(usize, usize, usize, usize)>,
    default_row: Option<f64>,
    default_col: Option<f64>,
    frozen_rows: usize,
    frozen_cols: usize,
    gridlines: bool,
    truncated: bool,
    data_rows: usize,
    data_cols: usize,
}

struct Context<'a> {
    xfs: &'a [Xf],
    shared: &'a [String],
    locale: &'a Locale,
    date1904: bool,
}

impl Context<'_> {
    /// The value a `<v>` (or inline string) holds for a cell type.
    fn value(&self, raw: &RawCell, format: Option<&str>) -> Option<Value> {
        let text = raw.value.as_deref();
        match raw.kind.as_str() {
            "inlineStr" => raw
                .inline
                .clone()
                .or_else(|| text.map(str::to_string))
                .map(Value::Text),
            "s" => {
                let index: usize = text?.trim().parse().ok()?;
                self.shared.get(index).cloned().map(Value::Text)
            }
            "str" => text.map(|text| Value::Text(text.to_string())),
            "b" => Some(Value::Bool(text?.trim() == "1")),
            "e" => Some(Value::Error(text?.to_string())),
            "d" => {
                let text = text?;
                Some(
                    DateTime::parse_iso(text)
                        .map_or_else(|| Value::Text(text.to_string()), Value::Date),
                )
            }
            _ => {
                let number: f64 = text?.trim().parse().ok().filter(|n: &f64| n.is_finite())?;
                if is_date_format(format) {
                    if let Some(date) = DateTime::from_serial(number, self.date1904) {
                        return Some(Value::Date(date));
                    }
                }
                Some(Value::Number(number))
            }
        }
    }

    fn cell(
        &self,
        raw: &RawCell,
        formula: Option<String>,
        row: usize,
        col: usize,
    ) -> Option<SheetCell> {
        let xf = self.xfs.get(raw.style);
        let format = xf.and_then(|xf| xf.num_fmt.as_deref());
        let style = xf.and_then(|xf| xf.style.clone());
        let value = self.value(raw, format);
        let mut out = SheetCell {
            row,
            col,
            ..SheetCell::default()
        };
        if let Some(formula) = formula {
            let formula = format!("={formula}");
            match value {
                None => {
                    out.text = formula.clone();
                    out.uncalculated = true;
                }
                Some(Value::Text(text)) if text.is_empty() => {}
                Some(result) => {
                    let shown = printed(&result, format, self.locale);
                    out.text = shown.text;
                    out.numeric = shown.numeric;
                    out.error = shown.error;
                    if let Value::Number(number) = result {
                        out.value = Some(number);
                    }
                }
            }
            out.input = formula;
        } else if let Some(value) = value {
            let shown = printed(&value, format, self.locale);
            out.text = shown.text;
            out.numeric = shown.numeric;
            out.error = shown.error;
            out.input = input_of(&value);
            if let Value::Number(number) = value {
                out.value = Some(number);
            }
        }
        out.style = style;
        (!out.text.is_empty() || out.style.is_some()).then_some(out)
    }
}

fn read_sheet_part(xml: &str, context: &Context<'_>) -> SheetParts {
    let mut parts = SheetParts {
        gridlines: true,
        ..SheetParts::default()
    };
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    config.check_end_names = false;
    config.allow_unmatched_ends = true;

    let mut shared_formulas: HashMap<String, (String, usize, usize)> = HashMap::new();
    let mut row_number = 0usize;
    let mut row_cells: Vec<SheetCell> = Vec::new();
    let mut row_width = 0usize;
    let mut cell: Option<RawCell> = None;
    // Which text element the characters belong to.
    let (mut in_v, mut in_f, mut in_is, mut in_t, mut in_rph) = (false, false, false, false, false);
    let mut seen_view = false;
    let mut next_row = 0usize;

    let begin_row =
        |start: &BytesStart<'_>, next_row: &mut usize, parts: &mut SheetParts| -> usize {
            let number = start_number(start, "r")
                .map_or(*next_row + 1, |n| n as usize)
                .max(1);
            *next_row = number;
            let height = if start_flag(start, "hidden") {
                0.0
            } else {
                start_number(start, "ht").filter(|h| *h > 0.0).map_or_else(
                    || row_pixels(parts.default_row.unwrap_or(DEFAULT_ROW_HEIGHT)),
                    row_pixels,
                )
            };
            parts.row_heights.insert(number.max(1) - 1, height);
            number
        };

    macro_rules! finish_cell {
        () => {
            if let Some(raw) = cell.take() {
                let (row, col) = parse_address(&raw.address)
                    .unwrap_or((row_number.saturating_sub(1), row_width));
                let formula = match (&raw.formula, &raw.shared_index) {
                    (Some(text), Some(si)) if !text.is_empty() => {
                        shared_formulas.insert(si.clone(), (text.clone(), row, col));
                        Some(text.clone())
                    }
                    // A cell that shares the master's formula, shifted to its place.
                    (_, Some(si)) => {
                        shared_formulas
                            .get(si)
                            .map(|(master, master_row, master_col)| {
                                shift_formula(
                                    master,
                                    row as i64 - *master_row as i64,
                                    col as i64 - *master_col as i64,
                                )
                            })
                    }
                    (Some(text), None) if !text.is_empty() => Some(text.clone()),
                    _ => None,
                };
                row_width = row_width.max(col + 1);
                if let Some(built) = context.cell(&raw, formula, row, col) {
                    row_cells.push(built);
                }
            }
        };
    }

    while let Ok(event) = reader.read_event() {
        match event {
            Event::Start(ref start) | Event::Empty(ref start) => {
                let empty = matches!(event, Event::Empty(_));
                let name = local(start.name().as_ref()).to_string();
                match name.as_str() {
                    "row" => {
                        row_number = begin_row(start, &mut next_row, &mut parts);
                        row_cells.clear();
                        row_width = 0;
                    }
                    "c" => {
                        finish_cell!();
                        let raw = RawCell {
                            address: start_attr(start, "r").unwrap_or_default(),
                            kind: start_attr(start, "t").unwrap_or_default(),
                            style: start_number(start, "s").map_or(0, |n| n as usize),
                            ..RawCell::default()
                        };
                        cell = Some(raw);
                        if empty {
                            finish_cell!();
                        }
                    }
                    "v" => {
                        // An empty `<v/>` is a value of nothing, not no value.
                        if let Some(raw) = cell.as_mut() {
                            raw.value.get_or_insert_with(String::new);
                        }
                        in_v = !empty;
                    }
                    "f" => {
                        if let Some(raw) = cell.as_mut() {
                            raw.shared_index = start_attr(start, "si");
                            raw.formula.get_or_insert_with(String::new);
                        }
                        in_f = !empty;
                    }
                    "is" => in_is = !empty,
                    "rPh" => in_rph = !empty,
                    "t" if in_is && !in_rph => in_t = !empty,
                    "mergeCell" => {
                        if let Some((top_left, bottom_right)) =
                            start_attr(start, "ref").as_deref().and_then(|r| {
                                r.split_once(':')
                                    .map(|(a, b)| (a.to_string(), b.to_string()))
                            })
                        {
                            if let (Some(a), Some(b)) =
                                (parse_address(&top_left), parse_address(&bottom_right))
                            {
                                parts.merges.push((
                                    a.0.min(b.0),
                                    a.1.min(b.1),
                                    a.0.max(b.0),
                                    a.1.max(b.1),
                                ));
                            }
                        }
                    }
                    "col" => {
                        let min = start_number(start, "min").map_or(1, |n| n as usize);
                        let max = start_number(start, "max").map_or(min, |n| n as usize);
                        parts.columns.push(ColumnDef {
                            min,
                            max: max.max(min),
                            width: start_number(start, "width"),
                            hidden: start_flag(start, "hidden"),
                        });
                    }
                    "sheetFormatPr" => {
                        parts.default_row =
                            start_number(start, "defaultRowHeight").filter(|h| *h > 0.0);
                        parts.default_col =
                            start_number(start, "defaultColWidth").filter(|w| *w > 0.0);
                    }
                    "sheetView" if !seen_view => {
                        seen_view = true;
                        parts.gridlines = !matches!(
                            start_attr(start, "showGridLines").as_deref(),
                            Some("0" | "false")
                        );
                    }
                    "pane"
                        if seen_view && start_attr(start, "state").as_deref() == Some("frozen") =>
                    {
                        parts.frozen_rows = start_number(start, "ySplit").map_or(0, |n| n as usize);
                        parts.frozen_cols = start_number(start, "xSplit").map_or(0, |n| n as usize);
                    }
                    _ => {}
                }
            }
            Event::End(ref end) => {
                let name = local(end.name().as_ref()).to_string();
                match name.as_str() {
                    "c" => finish_cell!(),
                    "v" => in_v = false,
                    "f" => in_f = false,
                    "is" => in_is = false,
                    "rPh" => in_rph = false,
                    "t" => in_t = false,
                    "row" => {
                        finish_cell!();
                        if parts.cells.len() + row_width > MAX_CELLS {
                            parts.truncated = true;
                            break;
                        }
                        for built in row_cells.drain(..) {
                            parts.data_rows = parts.data_rows.max(built.row + 1);
                            parts.data_cols = parts.data_cols.max(built.col + 1);
                            parts.cells.push(built);
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(ref text) => {
                if let Some(raw) = cell.as_mut() {
                    push_text(raw, text, in_v, in_f, in_t);
                }
            }
            Event::CData(ref data) => {
                if let Some(raw) = cell.as_mut() {
                    push_text(raw, data, in_v, in_f, in_t);
                }
            }
            Event::GeneralRef(ref reference) => {
                if let (Some(raw), Some(decoded)) = (cell.as_mut(), decode_reference(reference)) {
                    push_text(raw, &decoded, in_v, in_f, in_t);
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    parts
}

fn push_text(raw: &mut RawCell, text: &str, in_v: bool, in_f: bool, in_t: bool) {
    if in_v {
        raw.value.get_or_insert_with(String::new).push_str(text);
    } else if in_f {
        raw.formula.get_or_insert_with(String::new).push_str(text);
    } else if in_t {
        raw.inline.get_or_insert_with(String::new).push_str(text);
    }
}

/// Lay the cells out as a grid: padded to look like a sheet, with every
/// column and row sized.
#[allow(clippy::too_many_arguments)]
fn layout(
    name: String,
    data_rows: usize,
    data_cols: usize,
    cells: Vec<SheetCell>,
    col_widths: &HashMap<usize, f64>,
    row_heights: &HashMap<usize, f64>,
    frozen_rows: usize,
    frozen_cols: usize,
    gridlines: bool,
    truncated: bool,
    default_col: f64,
    default_row: f64,
) -> Sheet {
    let row_count = MIN_ROWS.max(data_rows + if truncated { 0 } else { PAD_ROWS });
    let col_count = MIN_COLS.max(data_cols + PAD_COLS);
    Sheet {
        name,
        row_count,
        col_count,
        col_widths: (0..col_count)
            .map(|index| col_widths.get(&index).copied().unwrap_or(default_col))
            .collect(),
        row_heights: (0..row_count)
            .map(|index| row_heights.get(&index).copied().unwrap_or(default_row))
            .collect(),
        cells,
        frozen_rows: frozen_rows.min(row_count),
        frozen_cols: frozen_cols.min(col_count),
        gridlines,
        truncated,
        drawings: None,
    }
}

fn build_sheet(name: String, mut parts: SheetParts) -> Sheet {
    let mut data_rows = parts.data_rows;
    let mut data_cols = parts.data_cols;
    // Cells under a merge other than its top-left corner are not drawn.
    let mut index: HashMap<(usize, usize), usize> = parts
        .cells
        .iter()
        .enumerate()
        .map(|(at, cell)| ((cell.row, cell.col), at))
        .collect();
    let mut hidden: HashSet<usize> = HashSet::new();
    let mut spans: Vec<(usize, usize, u32, u32)> = Vec::new();
    for &(top, left, bottom, right) in &parts.merges {
        if parts.truncated && top + 1 > data_rows {
            continue;
        }
        let area = (bottom + 1 - top).saturating_mul(right + 1 - left);
        if area <= parts.cells.len() {
            for row in top..=bottom {
                for col in left..=right {
                    if (row, col) != (top, left) {
                        if let Some(at) = index.get(&(row, col)) {
                            hidden.insert(*at);
                        }
                    }
                }
            }
        } else {
            for (at, cell) in parts.cells.iter().enumerate() {
                let inside =
                    (top..=bottom).contains(&cell.row) && (left..=right).contains(&cell.col);
                if inside && (cell.row, cell.col) != (top, left) {
                    hidden.insert(at);
                }
            }
        }
        spans.push((
            top,
            left,
            (bottom - top + 1) as u32,
            (right - left + 1) as u32,
        ));
        data_rows = data_rows.max(bottom + 1);
        data_cols = data_cols.max(right + 1);
    }
    let mut cells: Vec<SheetCell> = parts
        .cells
        .drain(..)
        .enumerate()
        .filter(|(at, _)| !hidden.contains(at))
        .map(|(_, cell)| cell)
        .collect();
    index = cells
        .iter()
        .enumerate()
        .map(|(at, cell)| ((cell.row, cell.col), at))
        .collect();
    for (top, left, row_span, col_span) in spans {
        let at = *index.entry((top, left)).or_insert_with(|| {
            cells.push(SheetCell {
                row: top,
                col: left,
                ..SheetCell::default()
            });
            cells.len() - 1
        });
        cells[at].row_span = Some(row_span);
        cells[at].col_span = Some(col_span);
    }
    let default_row = row_pixels(parts.default_row.unwrap_or(DEFAULT_ROW_HEIGHT));
    let default_col = column_pixels(parts.default_col.unwrap_or(DEFAULT_COL_WIDTH));
    let col_count = MIN_COLS.max(data_cols + PAD_COLS);
    let mut col_widths: HashMap<usize, f64> = HashMap::new();
    for definition in &parts.columns {
        // A later definition of the same column wins.
        for col in definition.min..=definition.max.min(col_count) {
            if col == 0 {
                continue;
            }
            let width = if definition.hidden {
                Some(0.0)
            } else {
                definition.width.filter(|w| *w > 0.0).map(column_pixels)
            };
            match width {
                Some(width) => {
                    col_widths.insert(col - 1, width);
                }
                None => {
                    col_widths.remove(&(col - 1));
                }
            }
        }
    }
    layout(
        name,
        data_rows,
        data_cols,
        cells,
        &col_widths,
        &parts.row_heights,
        parts.frozen_rows,
        parts.frozen_cols,
        parts.gridlines,
        parts.truncated,
        default_col,
        default_row,
    )
}

// ---------------------------------------------------------------- CSV

/// RFC 4180 rows: quoted fields may hold the delimiter, quotes and line breaks.
pub fn parse_delimited(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let chars: Vec<char> = text.chars().collect();
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut index = 0;
    while index < chars.len() {
        let ch = chars[index];
        if quoted {
            if ch == '"' {
                if chars.get(index + 1) == Some(&'"') {
                    field.push('"');
                    index += 1;
                } else {
                    quoted = false;
                }
            } else {
                field.push(ch);
            }
            index += 1;
            continue;
        }
        if ch == '"' && field.is_empty() {
            quoted = true;
        } else if ch == delimiter {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' || ch == '\r' {
            if ch == '\r' && chars.get(index + 1) == Some(&'\n') {
                index += 1;
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(ch);
        }
        index += 1;
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Comma, unless the first line is clearly split by semicolons or tabs.
pub fn sniff_delimiter(text: &str, path: &str) -> char {
    if extension_of(path) == ".tsv" {
        return '\t';
    }
    let line = text.split(['\n', '\r']).next().unwrap_or("");
    let count = |delimiter: char| line.matches(delimiter).count();
    let mut candidates = [',', ';', '\t'];
    candidates.sort_by_key(|c| std::cmp::Reverse(count(*c)));
    if count(candidates[0]) > 0 {
        candidates[0]
    } else {
        ','
    }
}

static NUMBER_TEXT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[-+]?(\d+([.,]\d+)?|\d{1,3}([ ,.]\d{3})+([.,]\d+)?)%?$").expect("a valid pattern")
});

pub fn csv_workbook(text: &str, path: &str) -> Workbook {
    let clean = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rows = parse_delimited(clean, sniff_delimiter(clean, path));
    let mut cells = Vec::new();
    let mut col_count = 0;
    let mut truncated = false;
    let mut row_count = 0;
    for (row, fields) in rows.iter().enumerate() {
        if cells.len() + fields.len() > MAX_CELLS {
            truncated = true;
            break;
        }
        row_count = row + 1;
        col_count = col_count.max(fields.len());
        for (col, value) in fields.iter().enumerate() {
            if !value.is_empty() {
                cells.push(SheetCell {
                    row,
                    col,
                    text: value.clone(),
                    input: value.clone(),
                    numeric: NUMBER_TEXT.is_match(value.trim()),
                    ..SheetCell::default()
                });
            }
        }
    }
    let stem = basename(path);
    let stem = stem.rfind('.').map_or(stem, |dot| &stem[..dot]);
    Workbook {
        sheets: vec![layout(
            if stem.is_empty() {
                "Sheet1".into()
            } else {
                stem.to_string()
            },
            row_count,
            col_count,
            cells,
            &HashMap::new(),
            &HashMap::new(),
            0,
            0,
            true,
            truncated,
            column_pixels(DEFAULT_COL_WIDTH),
            row_pixels(DEFAULT_ROW_HEIGHT),
        )],
    }
}

// ---------------------------------------------------------------- workbooks

const UNREADABLE: &str = "This file could not be read as a workbook";

fn unreadable(cause: &str) -> String {
    if cause.is_empty() {
        format!("{UNREADABLE}.")
    } else {
        format!("{UNREADABLE}: {cause}")
    }
}

/// Read a spreadsheet file into the sheets a viewer draws.
pub fn read_workbook(path: impl AsRef<Path>) -> Result<Workbook, String> {
    read_workbook_with(path, &Locale::default())
}

/// [`read_workbook`], printing numbers and dates in a locale.
pub fn read_workbook_with(path: impl AsRef<Path>, locale: &Locale) -> Result<Workbook, String> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    parse_workbook(bytes, &path.to_string_lossy(), locale)
}

/// Read a spreadsheet file's bytes; `path` says which format they are in.
pub fn parse_workbook(bytes: Vec<u8>, path: &str, locale: &Locale) -> Result<Workbook, String> {
    match extension_of(path).as_str() {
        ".csv" | ".tsv" => {
            return Ok(csv_workbook(&String::from_utf8_lossy(&bytes), path));
        }
        ".xls" => return Err(OLD_XLS.to_string()),
        _ => {}
    }
    let mut package = Package::from_bytes(bytes).map_err(|cause| unreadable(&cause))?;
    let workbook = package
        .xml("xl/workbook.xml")
        .ok_or_else(|| unreadable("it has no workbook part"))?;
    let rels = package.rels("xl/workbook.xml");
    let part_of = |kind: &str, fallback: &str| -> String {
        rels.iter()
            .find(|rel| rel.kind_name() == kind && !rel.external)
            .map_or_else(
                || fallback.to_string(),
                |rel| resolve_part("xl/workbook.xml", &rel.target),
            )
    };
    let theme_part = part_of("theme", "xl/theme/theme1.xml");
    let theme_xml = package
        .text(&theme_part)
        .map(|text| super::xml::parse_xml(&text));
    let theme = theme_colors(theme_xml.as_ref());
    let styles_part = part_of("styles", "xl/styles.xml");
    let styles_xml = package
        .text(&styles_part)
        .map(|text| super::xml::parse_xml(&text));
    let xfs = read_styles(styles_xml.as_ref(), &theme);
    let strings_part = part_of("sharedStrings", "xl/sharedStrings.xml");
    let strings_xml = package
        .text(&strings_part)
        .map(|text| super::xml::parse_xml(&text));
    let shared = read_shared_strings(strings_xml.as_ref());
    let context = Context {
        xfs: &xfs,
        shared: &shared,
        locale,
        date1904: workbook
            .find("workbookPr")
            .is_some_and(|pr| matches!(pr.attr("date1904"), Some("1" | "true"))),
    };

    struct Read {
        sheet: Sheet,
        part: String,
        hidden: bool,
    }
    let mut read: Vec<Read> = Vec::new();
    // Hidden sheets are read too: a chart may draw from one.
    for entry in workbook.find_all("sheet") {
        let name = entry.attr("name").unwrap_or("").to_string();
        let part = entry
            .rel_attr("id")
            .and_then(|id| rels.get(id))
            .map(|rel| resolve_part("xl/workbook.xml", &rel.target))
            .unwrap_or_default();
        let xml = package.text(&part).unwrap_or_default();
        read.push(Read {
            sheet: build_sheet(name, read_sheet_part(&xml, &context)),
            part,
            hidden: matches!(entry.attr("state"), Some("hidden" | "veryHidden")),
        });
    }
    if read.iter().all(|entry| entry.hidden) {
        return Err("This workbook has no visible sheets.".to_string());
    }
    let mut sheets: Vec<Sheet> = read.iter().map(|entry| entry.sheet.clone()).collect();
    let parts: Vec<String> = read.iter().map(|entry| entry.part.clone()).collect();
    // Without its pictures and charts the sheet still reads.
    drawings::read_drawings(&mut package, &mut sheets, &parts, &theme);
    Ok(Workbook {
        sheets: sheets
            .into_iter()
            .zip(read.iter())
            .filter(|(_, entry)| !entry.hidden)
            .map(|(sheet, _)| sheet)
            .collect(),
    })
}

#[cfg(test)]
#[path = "xlsx_tests.rs"]
mod tests;
