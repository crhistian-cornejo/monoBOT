//! A Word document read for showing, not editing: paragraphs with their
//! headings, alignment and list markers, runs with their formatting and
//! links, inline pictures, tables, and the page setup.
//!
//! The Tauri viewer hands a `.docx` to `docx-preview`, which draws it as HTML
//! and exposes no model, so this one is new: it reads the same package parts
//! (`document.xml`, `styles.xml`, `numbering.xml`, the headers and footers,
//! the media) into plain structs a native view can lay out.
use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::xml::{resolve_part, Package, Rels, XmlNode};

const NOT_A_DOCUMENT: &str = "This is not a Word document.";
/// A picture larger than this is left out rather than loaded.
const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;
/// Past this many top-level blocks the rest are left out.
pub const MAX_BLOCKS: usize = 100_000;
/// Tables nested deeper than this are left out.
const MAX_DEPTH: usize = 12;

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VerticalAlign {
    Superscript,
    Subscript,
}

/// A picture sitting in the text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InlineImage {
    pub mime: String,
    pub bytes: Vec<u8>,
    /// EMU, as the file measures; 0 when the file does not say.
    pub width: f64,
    pub height: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alt: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Run {
    /// `\t` for a tab, `\n` for a line break.
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strike: bool,
    /// Points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    /// `#rrggbb`; absent is the automatic color.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<VerticalAlign>,
    /// Where a hyperlink goes: a URL, or `#bookmark` inside the document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<InlineImage>,
}

impl Run {
    /// Whether two runs print alike, so their text can be one run.
    fn same_look(&self, other: &Run) -> bool {
        self.image.is_none()
            && other.image.is_none()
            && Run {
                text: String::new(),
                ..self.clone()
            } == Run {
                text: String::new(),
                ..other.clone()
            }
    }
}

/// A paragraph's place in a list.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    /// Zero-based nesting.
    pub level: u32,
    pub ordered: bool,
    /// What precedes the text: `•`, `1.`, `a)`, `1.2.`
    pub marker: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Paragraph {
    /// The style's id in the file, such as `Heading1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    /// 1 to 9 for a heading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ListItem>,
    /// Points of left indent, when the paragraph has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub indent: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub page_break_before: bool,
    pub runs: Vec<Run>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TableCell {
    pub col_span: u32,
    pub row_span: u32,
    /// `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TableRow {
    /// A repeating header row.
    #[serde(default, skip_serializing_if = "is_false")]
    pub header: bool,
    pub cells: Vec<TableCell>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Table {
    /// Column widths in points.
    pub columns: Vec<f64>,
    pub rows: Vec<TableRow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Block {
    Paragraph(Paragraph),
    Table(Table),
}

/// The page, in points.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PageSetup {
    pub width: f64,
    pub height: f64,
    pub margin_top: f64,
    pub margin_right: f64,
    pub margin_bottom: f64,
    pub margin_left: f64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct DocxDocument {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<PageSetup>,
    pub blocks: Vec<Block>,
    /// The default header and footer of the last section.
    pub header: Vec<Block>,
    pub footer: Vec<Block>,
    /// The document had more blocks than the viewer reads.
    #[serde(default, skip_serializing_if = "is_false")]
    pub truncated: bool,
}

// ---------------------------------------------------------------- styles

#[derive(Clone, Debug, Default, PartialEq)]
struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    size: Option<f64>,
    color: Option<Option<String>>,
    font: Option<String>,
    highlight: Option<String>,
    vertical_align: Option<Option<VerticalAlign>>,
}

/// `<w:b/>` is on, `<w:b w:val="0"/>` off, absent is unsaid.
fn toggle(rpr: &XmlNode, name: &str) -> Option<bool> {
    let node = rpr.child(name)?;
    Some(!matches!(node.attr("val"), Some("0" | "false" | "off")))
}

fn hex_color(value: &str) -> Option<String> {
    (value.len() == 6 && value.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{}", value.to_lowercase()))
}

fn highlight_color(name: &str) -> Option<String> {
    Some(
        match name {
            "yellow" => "#ffff00",
            "green" => "#00ff00",
            "cyan" => "#00ffff",
            "magenta" => "#ff00ff",
            "blue" => "#0000ff",
            "red" => "#ff0000",
            "darkBlue" => "#000080",
            "darkCyan" => "#008080",
            "darkGreen" => "#008000",
            "darkMagenta" => "#800080",
            "darkRed" => "#800000",
            "darkYellow" => "#808000",
            "darkGray" => "#808080",
            "lightGray" => "#c0c0c0",
            "black" => "#000000",
            "white" => "#ffffff",
            _ => return None,
        }
        .to_string(),
    )
}

impl RunProps {
    fn from_node(rpr: Option<&XmlNode>) -> RunProps {
        let Some(rpr) = rpr else {
            return RunProps::default();
        };
        RunProps {
            bold: toggle(rpr, "b"),
            italic: toggle(rpr, "i"),
            underline: rpr
                .child("u")
                .map(|u| !matches!(u.attr("val"), Some("none"))),
            strike: toggle(rpr, "strike").or_else(|| toggle(rpr, "dstrike")),
            size: rpr
                .child("sz")
                .and_then(|n| n.number("val"))
                .map(|half| half / 2.0),
            color: rpr
                .child("color")
                .map(|c| c.attr("val").and_then(hex_color)),
            font: rpr
                .child("rFonts")
                .and_then(|f| f.attr("ascii").or_else(|| f.attr("hAnsi")))
                .map(str::to_string),
            highlight: rpr
                .child("highlight")
                .and_then(|h| h.attr("val"))
                .and_then(highlight_color)
                .or_else(|| {
                    rpr.child("shd")
                        .and_then(|s| s.attr("fill"))
                        .and_then(hex_color)
                }),
            vertical_align: rpr.child("vertAlign").map(|v| match v.attr("val") {
                Some("superscript") => Some(VerticalAlign::Superscript),
                Some("subscript") => Some(VerticalAlign::Subscript),
                _ => None,
            }),
        }
    }

    /// These properties over a base's: what this says wins.
    fn over(self, base: RunProps) -> RunProps {
        RunProps {
            bold: self.bold.or(base.bold),
            italic: self.italic.or(base.italic),
            underline: self.underline.or(base.underline),
            strike: self.strike.or(base.strike),
            size: self.size.or(base.size),
            color: self.color.or(base.color),
            font: self.font.or(base.font),
            highlight: self.highlight.or(base.highlight),
            vertical_align: self.vertical_align.or(base.vertical_align),
        }
    }
}

fn align_of(code: &str) -> Option<Align> {
    match code {
        "left" | "start" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" | "end" => Some(Align::Right),
        "both" | "distribute" | "justify" => Some(Align::Justify),
        _ => None,
    }
}

/// How a style's paragraph looks, before the style it is based on.
#[derive(Clone, Debug, Default)]
struct StyleDef {
    name: String,
    based_on: Option<String>,
    run: RunProps,
    align: Option<Align>,
    outline: Option<u8>,
    indent: Option<f64>,
    numbering: Option<(u32, u32)>,
}

#[derive(Default)]
struct Styles {
    defaults: RunProps,
    by_id: HashMap<String, StyleDef>,
}

/// A paragraph's `numPr`: the list and the level.
fn numbering_of(ppr: Option<&XmlNode>) -> Option<(u32, u32)> {
    let num_pr = ppr?.child("numPr")?;
    let id = num_pr.child("numId").and_then(|n| n.number("val"))? as u32;
    let level = num_pr
        .child("ilvl")
        .and_then(|n| n.number("val"))
        .unwrap_or(0.0)
        .max(0.0) as u32;
    Some((id, level))
}

fn indent_of(ppr: Option<&XmlNode>) -> Option<f64> {
    let ind = ppr?.child("ind")?;
    ind.number("left")
        .or_else(|| ind.number("start"))
        .filter(|twips| *twips > 0.0)
        .map(|twips| twips / 20.0)
}

impl Styles {
    fn read(root: Option<&XmlNode>) -> Styles {
        let Some(root) = root else {
            return Styles::default();
        };
        let defaults = RunProps::from_node(root.path(&["docDefaults", "rPrDefault", "rPr"]));
        let by_id = root
            .children_named("style")
            .filter_map(|style| {
                let id = style.attr("styleId")?.to_string();
                let ppr = style.child("pPr");
                Some((
                    id,
                    StyleDef {
                        name: style
                            .child("name")
                            .and_then(|n| n.attr("val"))
                            .unwrap_or("")
                            .to_string(),
                        based_on: style
                            .child("basedOn")
                            .and_then(|n| n.attr("val"))
                            .map(str::to_string),
                        run: RunProps::from_node(style.child("rPr")),
                        align: ppr
                            .and_then(|p| p.child("jc"))
                            .and_then(|j| j.attr("val"))
                            .and_then(align_of),
                        outline: ppr
                            .and_then(|p| p.child("outlineLvl"))
                            .and_then(|o| o.number("val"))
                            .filter(|level| *level < 9.0)
                            .map(|level| level as u8 + 1),
                        indent: indent_of(ppr),
                        numbering: numbering_of(ppr),
                    },
                ))
            })
            .collect();
        Styles { defaults, by_id }
    }

    /// A style and the styles it is based on, the style itself first.
    fn chain(&self, id: &str) -> Vec<&StyleDef> {
        let mut out = Vec::new();
        let mut next = Some(id);
        while let Some(current) = next {
            let Some(style) = self.by_id.get(current) else {
                break;
            };
            if out.len() >= 16 {
                break;
            }
            out.push(style);
            next = style.based_on.as_deref();
        }
        out
    }

    /// The run properties a style gives, over its bases.
    fn run_props(&self, id: Option<&str>) -> RunProps {
        let mut props = RunProps::default();
        if let Some(id) = id {
            for style in self.chain(id) {
                props = props.over(style.run.clone());
            }
        }
        props
    }
}

/// `Heading 3` and `heading3` are level 3.
fn heading_level(name: &str) -> Option<u8> {
    let lower = name.to_lowercase();
    let digits = lower.strip_prefix("heading")?.trim();
    digits
        .parse::<u8>()
        .ok()
        .filter(|level| (1..=9).contains(level))
}

// ---------------------------------------------------------------- numbering

#[derive(Clone, Debug, Default)]
struct Level {
    format: String,
    text: String,
    start: u32,
}

#[derive(Default)]
struct Numbering {
    /// numId to abstract numbering id.
    nums: HashMap<u32, u32>,
    abstracts: HashMap<u32, HashMap<u32, Level>>,
}

impl Numbering {
    fn read(root: Option<&XmlNode>) -> Numbering {
        let Some(root) = root else {
            return Numbering::default();
        };
        let abstracts = root
            .children_named("abstractNum")
            .filter_map(|abs| {
                let id = abs.number("abstractNumId")? as u32;
                let levels = abs
                    .children_named("lvl")
                    .filter_map(|lvl| {
                        Some((
                            lvl.number("ilvl")? as u32,
                            Level {
                                format: lvl
                                    .child("numFmt")
                                    .and_then(|n| n.attr("val"))
                                    .unwrap_or("decimal")
                                    .to_string(),
                                text: lvl
                                    .child("lvlText")
                                    .and_then(|n| n.attr("val"))
                                    .unwrap_or("")
                                    .to_string(),
                                start: lvl
                                    .child("start")
                                    .and_then(|n| n.number("val"))
                                    .unwrap_or(1.0) as u32,
                            },
                        ))
                    })
                    .collect();
                Some((id, levels))
            })
            .collect();
        let nums = root
            .children_named("num")
            .filter_map(|num| {
                Some((
                    num.number("numId")? as u32,
                    num.child("abstractNumId")?.number("val")? as u32,
                ))
            })
            .collect();
        Numbering { nums, abstracts }
    }

    fn level(&self, num_id: u32, level: u32) -> Option<&Level> {
        self.abstracts.get(self.nums.get(&num_id)?)?.get(&level)
    }
}

fn roman(mut value: u32, upper: bool) -> String {
    const TABLE: [(u32, &str); 13] = [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ];
    let mut out = String::new();
    for (amount, letters) in TABLE {
        while value >= amount {
            out.push_str(letters);
            value -= amount;
        }
    }
    if upper {
        out.to_uppercase()
    } else {
        out
    }
}

/// A counter in a level's number format: Word repeats a letter past `z`.
fn counter_text(format: &str, value: u32) -> String {
    match format {
        "lowerLetter" | "upperLetter" => {
            let letter = char::from(b'a' + ((value.max(1) - 1) % 26) as u8);
            let text: String =
                std::iter::repeat_n(letter, ((value.max(1) - 1) / 26 + 1) as usize).collect();
            if format == "upperLetter" {
                text.to_uppercase()
            } else {
                text
            }
        }
        "lowerRoman" => roman(value, false),
        "upperRoman" => roman(value, true),
        "decimalZero" => format!("{value:02}"),
        "none" => String::new(),
        _ => value.to_string(),
    }
}

/// A bullet written in a symbol font is a private-use character the viewer's
/// fonts do not have.
fn bullet_text(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| {
            if ('\u{f000}'..='\u{f0ff}').contains(&c) {
                '•'
            } else {
                c
            }
        })
        .collect();
    if cleaned.trim().is_empty() {
        "•".to_string()
    } else {
        cleaned
    }
}

// ---------------------------------------------------------------- reading

fn image_mime(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        _ => return None,
    })
}

/// A table row as read: whether it repeats as a header, and its cells as
/// `(start column, continues the cell above, the cell)`.
type GridRow = (bool, Vec<(usize, bool, TableCell)>);

struct Reader {
    package: Package,
    styles: Styles,
    numbering: Numbering,
    /// The part being read and its relationships.
    part: String,
    rels: Rels,
    /// Counters per list and level, for the markers.
    counters: HashMap<u32, Vec<u32>>,
}

impl Reader {
    fn image(&mut self, rel_id: &str) -> Option<(String, Vec<u8>)> {
        let rel = self.rels.get(rel_id).filter(|rel| !rel.external)?;
        let part = resolve_part(&self.part, &rel.target);
        let mime = image_mime(&part.rsplit('.').next()?.to_lowercase())?;
        let bytes = self
            .package
            .bytes(&part)
            .filter(|b| b.len() <= MAX_IMAGE_BYTES)?;
        Some((mime.to_string(), bytes))
    }

    fn marker(&mut self, num_id: u32, level: u32) -> Option<ListItem> {
        if num_id == 0 {
            return None;
        }
        let definition = self.numbering.level(num_id, level)?.clone();
        let bullet = definition.format == "bullet";
        let counters = self.counters.entry(num_id).or_default();
        if counters.len() <= level as usize {
            counters.resize(level as usize + 1, 0);
        }
        let own = &mut counters[level as usize];
        *own = if *own == 0 {
            definition.start
        } else {
            *own + 1
        };
        counters.truncate(level as usize + 1);
        let current = counters.clone();
        let marker = if bullet {
            bullet_text(&definition.text)
        } else {
            let mut text = definition.text.clone();
            for slot in 0..=level {
                let format = self
                    .numbering
                    .level(num_id, slot)
                    .map_or("decimal", |l| l.format.as_str())
                    .to_string();
                let value = current.get(slot as usize).copied().unwrap_or(1).max(1);
                text = text.replace(&format!("%{}", slot + 1), &counter_text(&format, value));
            }
            text
        };
        Some(ListItem {
            level,
            ordered: !bullet && definition.format != "none",
            marker,
        })
    }

    fn collect_runs(
        &mut self,
        node: &XmlNode,
        link: Option<&str>,
        base: &RunProps,
        out: &mut Vec<Run>,
        depth: usize,
    ) {
        if depth > MAX_DEPTH {
            return;
        }
        for entry in &node.children {
            match entry.name.as_str() {
                "r" => self.read_run(entry, link, base, out),
                "hyperlink" => {
                    let target = entry
                        .rel_attr("id")
                        .and_then(|id| self.rels.get(id))
                        .map(|rel| rel.target.clone())
                        .or_else(|| entry.attr("anchor").map(|a| format!("#{a}")));
                    self.collect_runs(entry, target.as_deref().or(link), base, out, depth + 1);
                }
                "fldSimple" | "smartTag" | "ins" | "customXml" => {
                    self.collect_runs(entry, link, base, out, depth + 1);
                }
                "sdt" => {
                    if let Some(content) = entry.child("sdtContent") {
                        self.collect_runs(content, link, base, out, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn read_run(&mut self, run: &XmlNode, link: Option<&str>, base: &RunProps, out: &mut Vec<Run>) {
        let rpr = run.child("rPr");
        let style = rpr
            .and_then(|r| r.child("rStyle"))
            .and_then(|s| s.attr("val"));
        let props = RunProps::from_node(rpr)
            .over(self.styles.run_props(style))
            .over(base.clone());
        let look = Run {
            text: String::new(),
            bold: props.bold.unwrap_or(false),
            italic: props.italic.unwrap_or(false),
            underline: props.underline.unwrap_or(false),
            strike: props.strike.unwrap_or(false),
            size: props.size,
            color: props.color.flatten(),
            highlight: props.highlight,
            font: props.font,
            vertical_align: props.vertical_align.flatten(),
            link: link.map(str::to_string),
            image: None,
        };
        let mut text = String::new();
        let flush = |text: &mut String, out: &mut Vec<Run>| {
            if text.is_empty() {
                return;
            }
            let run = Run {
                text: std::mem::take(text),
                ..look.clone()
            };
            match out.last_mut() {
                Some(last) if last.same_look(&run) => last.text.push_str(&run.text),
                _ => out.push(run),
            }
        };
        for entry in &run.children {
            match entry.name.as_str() {
                "t" => text.push_str(&entry.text),
                "tab" => text.push('\t'),
                "br" | "cr" => text.push('\n'),
                "noBreakHyphen" => text.push('-'),
                "drawing" => {
                    flush(&mut text, out);
                    if let Some(image) = self.drawing(entry) {
                        out.push(Run {
                            image: Some(image),
                            ..look.clone()
                        });
                    }
                }
                "pict" => {
                    flush(&mut text, out);
                    let id = entry.find("imagedata").and_then(|d| d.rel_attr("id"));
                    if let Some((mime, bytes)) = id.and_then(|id| self.image(id)) {
                        out.push(Run {
                            image: Some(InlineImage {
                                mime,
                                bytes,
                                width: 0.0,
                                height: 0.0,
                                alt: None,
                            }),
                            ..look.clone()
                        });
                    }
                }
                _ => {}
            }
        }
        flush(&mut text, out);
    }

    fn drawing(&mut self, drawing: &XmlNode) -> Option<InlineImage> {
        let embed = drawing.find("blip")?.rel_attr("embed")?;
        let (mime, bytes) = self.image(embed)?;
        let extent = drawing.find("extent");
        let props = drawing.find("docPr");
        Some(InlineImage {
            mime,
            bytes,
            width: extent.and_then(|e| e.number("cx")).unwrap_or(0.0),
            height: extent.and_then(|e| e.number("cy")).unwrap_or(0.0),
            alt: props
                .and_then(|p| {
                    p.attr("descr")
                        .filter(|d| !d.is_empty())
                        .or_else(|| p.attr("name"))
                })
                .map(str::to_string),
        })
    }

    fn paragraph(&mut self, p: &XmlNode) -> Paragraph {
        let ppr = p.child("pPr");
        let style_id = ppr
            .and_then(|p| p.child("pStyle"))
            .and_then(|s| s.attr("val"))
            .map(str::to_string);
        let chain = style_id
            .as_deref()
            .map(|id| self.styles.chain(id))
            .unwrap_or_default();
        let named_level = chain
            .iter()
            .find_map(|style| heading_level(&style.name))
            .or_else(|| style_id.as_deref().and_then(heading_level));
        let outline = ppr
            .and_then(|p| p.child("outlineLvl"))
            .and_then(|o| o.number("val"))
            .filter(|level| *level < 9.0)
            .map(|level| level as u8 + 1)
            .or_else(|| chain.iter().find_map(|style| style.outline));
        let heading = named_level.or(outline);
        let align = ppr
            .and_then(|p| p.child("jc"))
            .and_then(|j| j.attr("val"))
            .and_then(align_of)
            .or_else(|| chain.iter().find_map(|style| style.align));
        let numbering =
            numbering_of(ppr).or_else(|| chain.iter().find_map(|style| style.numbering));
        let indent = indent_of(ppr).or_else(|| chain.iter().find_map(|style| style.indent));
        let base = {
            let mut props = RunProps::default();
            for style in chain.iter().rev() {
                props = style.run.clone().over(props);
            }
            // The paragraph mark's own formatting is not the text's; defaults sit under everything.
            props.over(self.styles.defaults.clone())
        };
        let mut runs = Vec::new();
        self.collect_runs(p, None, &base, &mut runs, 0);
        let list = numbering.and_then(|(id, level)| self.marker(id, level));
        Paragraph {
            style: style_id,
            heading,
            align,
            list,
            indent,
            page_break_before: ppr.and_then(|p| p.child("pageBreakBefore")).is_some(),
            runs,
        }
    }

    fn table(&mut self, tbl: &XmlNode, depth: usize) -> Table {
        let columns: Vec<f64> = tbl
            .child("tblGrid")
            .map(|grid| {
                grid.children_named("gridCol")
                    .map(|col| col.number("w").unwrap_or(0.0) / 20.0)
                    .collect()
            })
            .unwrap_or_default();
        // (start column, continues the cell above, the cell)
        let mut rows: Vec<GridRow> = Vec::new();
        for tr in tbl.children_named("tr") {
            let header = tr.path(&["trPr", "tblHeader"]).is_some();
            let mut cells = Vec::new();
            let mut column = 0usize;
            for tc in tr.children_named("tc") {
                let props = tc.child("tcPr");
                let span = props
                    .and_then(|p| p.child("gridSpan"))
                    .and_then(|g| g.number("val"))
                    .unwrap_or(1.0)
                    .max(1.0) as u32;
                let merge = props.and_then(|p| p.child("vMerge"));
                let continues = merge.is_some_and(|m| m.attr("val") != Some("restart"));
                let fill = props
                    .and_then(|p| p.child("shd"))
                    .and_then(|s| s.attr("fill"))
                    .and_then(hex_color);
                let blocks = self.blocks(tc, depth + 1);
                cells.push((
                    column,
                    continues,
                    TableCell {
                        col_span: span,
                        row_span: 1,
                        fill,
                        blocks,
                    },
                ));
                column += span as usize;
            }
            rows.push((header, cells));
        }
        // A cell that continues the one above is the taller cell's, not its own.
        for row in 0..rows.len() {
            for cell in 0..rows[row].1.len() {
                let (column, continues, _) = &rows[row].1[cell];
                if *continues {
                    continue;
                }
                let column = *column;
                let mut below = row + 1;
                let mut span = 1;
                while below < rows.len()
                    && rows[below]
                        .1
                        .iter()
                        .any(|(start, continues, _)| *start == column && *continues)
                {
                    span += 1;
                    below += 1;
                }
                rows[row].1[cell].2.row_span = span;
            }
        }
        Table {
            columns,
            rows: rows
                .into_iter()
                .map(|(header, cells)| TableRow {
                    header,
                    cells: cells
                        .into_iter()
                        .filter(|(_, continues, _)| !continues)
                        .map(|(_, _, cell)| cell)
                        .collect(),
                })
                .collect(),
        }
    }

    /// The blocks under a body, a cell, a header or a content control.
    fn blocks(&mut self, parent: &XmlNode, depth: usize) -> Vec<Block> {
        let mut out = Vec::new();
        if depth > MAX_DEPTH {
            return out;
        }
        for entry in &parent.children {
            if out.len() >= MAX_BLOCKS {
                break;
            }
            match entry.name.as_str() {
                "p" => out.push(Block::Paragraph(self.paragraph(entry))),
                "tbl" => out.push(Block::Table(self.table(entry, depth))),
                "sdt" => {
                    if let Some(content) = entry.child("sdtContent") {
                        out.extend(self.blocks(content, depth + 1));
                    }
                }
                "customXml" | "ins" => out.extend(self.blocks(entry, depth + 1)),
                _ => {}
            }
        }
        out
    }

    /// Another part's blocks (a header), read against its own relationships.
    fn part_blocks(&mut self, part: &str) -> Vec<Block> {
        let Some(root) = self.package.xml(part) else {
            return Vec::new();
        };
        let saved_part = std::mem::replace(&mut self.part, part.to_string());
        let saved_rels = std::mem::replace(&mut self.rels, self.package.rels(part));
        let blocks = self.blocks(&root, 0);
        self.part = saved_part;
        self.rels = saved_rels;
        blocks
    }
}

fn page_setup(section: &XmlNode) -> Option<PageSetup> {
    let size = section.child("pgSz")?;
    let margins = section.child("pgMar");
    let points = |node: Option<&XmlNode>, name: &str, fallback: f64| {
        node.and_then(|n| n.number(name)).unwrap_or(fallback) / 20.0
    };
    let (mut width, mut height) = (
        points(Some(size), "w", 12_240.0),
        points(Some(size), "h", 15_840.0),
    );
    // A landscape page whose size was written upright.
    if size.attr("orient") == Some("landscape") && width < height {
        std::mem::swap(&mut width, &mut height);
    }
    Some(PageSetup {
        width,
        height,
        margin_top: points(margins, "top", 1440.0),
        margin_right: points(margins, "right", 1440.0),
        margin_bottom: points(margins, "bottom", 1440.0),
        margin_left: points(margins, "left", 1440.0),
    })
}

/// Reads a .docx (or .docm) into its blocks, in reading order.
pub fn read_docx(path: impl AsRef<Path>) -> Result<DocxDocument, String> {
    let bytes = std::fs::read(path.as_ref()).map_err(|error| error.to_string())?;
    parse_docx(bytes)
}

/// Reads a .docx's bytes into its blocks.
pub fn parse_docx(bytes: Vec<u8>) -> Result<DocxDocument, String> {
    let mut package = Package::from_bytes(bytes).map_err(|_| NOT_A_DOCUMENT.to_string())?;
    let main = "word/document.xml";
    let document = package
        .xml(main)
        .ok_or_else(|| NOT_A_DOCUMENT.to_string())?;
    let rels = package.rels(main);
    let related = |kind: &str, fallback: &str| -> String {
        rels.iter()
            .find(|rel| rel.kind_name() == kind && !rel.external)
            .map_or_else(
                || fallback.to_string(),
                |rel| resolve_part(main, &rel.target),
            )
    };
    let styles_part = related("styles", "word/styles.xml");
    let numbering_part = related("numbering", "word/numbering.xml");
    let styles = Styles::read(package.xml(&styles_part).as_ref());
    let numbering = Numbering::read(package.xml(&numbering_part).as_ref());
    let title = package.xml("docProps/core.xml").and_then(|core| {
        core.find("title")
            .map(|t| t.text.trim().to_string())
            .filter(|t| !t.is_empty())
    });
    let mut reader = Reader {
        package,
        styles,
        numbering,
        part: main.to_string(),
        rels: rels.clone(),
        counters: HashMap::new(),
    };
    let body = document.child("body");
    let blocks = body.map(|b| reader.blocks(b, 0)).unwrap_or_default();
    let truncated = blocks.len() >= MAX_BLOCKS;
    let section = body.and_then(|b| b.child("sectPr"));
    let mut header = Vec::new();
    let mut footer = Vec::new();
    if let Some(section) = section {
        for (tag, into) in [
            ("headerReference", &mut header),
            ("footerReference", &mut footer),
        ] {
            let reference = section
                .children_named(tag)
                .find(|r| r.attr("type") == Some("default"));
            let target = reference
                .and_then(|r| r.rel_attr("id"))
                .and_then(|id| rels.get(id))
                .map(|rel| resolve_part(main, &rel.target));
            if let Some(part) = target {
                *into = reader.part_blocks(&part);
            }
        }
    }
    Ok(DocxDocument {
        title,
        page: section.and_then(page_setup),
        blocks,
        header,
        footer,
        truncated,
    })
}

#[cfg(test)]
#[path = "docx_tests.rs"]
mod tests;
