//! A PowerPoint deck read for showing, not editing: each slide's boxes where
//! the file puts them (in EMU, as the file measures), their text with its
//! sizes and colors, pictures, tables and the background. What it cannot
//! draw (charts, SmartArt, video) shows as a labelled box. Ported from the
//! Tauri viewer's `pptx.ts`.
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use super::xml::{number, resolve_part, Package, XmlNode};

/// A picture's bytes and what they are.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImageData {
    /// `image/png`, `image/jpeg`, `image/svg+xml`...
    pub mime: String,
    pub bytes: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Deck {
    /// EMU, as `<p:sldSz>` says.
    pub width: f64,
    pub height: f64,
    pub slides: Vec<Slide>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    /// One-based position in the deck's order, counting hidden slides.
    pub number: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_image: Option<ImageData>,
    pub items: Vec<SlideItem>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Degrees.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotate: Option<f64>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_h: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub flip_v: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Run {
    pub text: String,
    /// Points.
    pub size: f64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Paragraph {
    pub runs: Vec<Run>,
    pub align: Align,
    pub level: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bullet: Option<String>,
    /// Points, for an empty line's height.
    pub size: f64,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TextBody {
    pub paragraphs: Vec<Paragraph>,
    pub anchor: Anchor,
    /// Top, right, bottom, left, in EMU.
    pub inset: [f64; 4],
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TableCell {
    pub span: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    pub text: TextBody,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TableRow {
    pub height: f64,
    pub cells: Vec<TableCell>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShapeGeometry {
    Ellipse,
    Round,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShapeItem {
    #[serde(flatten)]
    pub frame: Frame,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<ShapeGeometry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<TextBody>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ImageItem {
    #[serde(flatten)]
    pub frame: Frame,
    pub image: ImageData,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct TableItem {
    #[serde(flatten)]
    pub frame: Frame,
    pub columns: Vec<f64>,
    pub rows: Vec<TableRow>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct UnsupportedItem {
    #[serde(flatten)]
    pub frame: Frame,
    pub label: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SlideItem {
    Shape(ShapeItem),
    Image(ImageItem),
    Table(TableItem),
    Unsupported(UnsupportedItem),
}

/// Past this many slides the rest are left out; the app opens it whole.
pub const MAX_SLIDES: usize = 300;
/// A picture larger than this is left out rather than loaded.
const MAX_IMAGE_BYTES: usize = 15 * 1024 * 1024;
const DEFAULT_SIZE: f64 = 18.0;
/// Groups nested deeper than this are left out.
const MAX_DEPTH: usize = 24;

const NOT_A_DECK: &str = "This is not a PowerPoint deck.";

fn image_mime(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

// ---- Parts and their relationships.

/// `(id, kind, resolved target)`, internal relationships only.
type PartRels = Rc<Vec<(String, String, String)>>;

struct Deckage {
    package: Package,
    parts: HashMap<String, Option<Rc<XmlNode>>>,
    rels: HashMap<String, PartRels>,
}

impl Deckage {
    fn part(&mut self, path: &str) -> Option<Rc<XmlNode>> {
        if let Some(found) = self.parts.get(path) {
            return found.clone();
        }
        let parsed = self.package.xml(path).map(Rc::new);
        self.parts.insert(path.to_string(), parsed.clone());
        parsed
    }

    fn relationships(&mut self, path: &str) -> PartRels {
        if let Some(found) = self.rels.get(path) {
            return found.clone();
        }
        let rels = self.package.rels(path);
        let list: Vec<(String, String, String)> = rels
            .iter_with_ids()
            .filter(|(_, rel)| !rel.external)
            .map(|(id, rel)| {
                (
                    id.to_string(),
                    rel.kind_name().to_string(),
                    resolve_part(path, &rel.target),
                )
            })
            .collect();
        let shared = Rc::new(list);
        self.rels.insert(path.to_string(), shared.clone());
        shared
    }

    fn related(&mut self, path: &str, kind: &str) -> Option<String> {
        self.relationships(path)
            .iter()
            .find(|(_, rel_kind, _)| rel_kind == kind)
            .map(|(_, _, target)| target.clone())
    }

    fn target(&mut self, path: &str, id: &str) -> Option<String> {
        self.relationships(path)
            .iter()
            .find(|(rel_id, _, _)| rel_id == id)
            .map(|(_, _, target)| target.clone())
    }

    fn image(&mut self, path: &str) -> Option<ImageData> {
        let extension = path.rsplit('.').next()?.to_lowercase();
        let mime = image_mime(&extension)?;
        let bytes = self.package.bytes(path)?;
        (bytes.len() <= MAX_IMAGE_BYTES).then(|| ImageData {
            mime: mime.to_string(),
            bytes,
        })
    }
}

// ---- XML, by local name: the prefixes (p:, a:, r:) vary between writers.

fn child_path<'a>(el: Option<&'a XmlNode>, path: &[&str]) -> Option<&'a XmlNode> {
    el?.path(path)
}

fn attr<'a>(el: Option<&'a XmlNode>, name: &str) -> Option<&'a str> {
    el?.attr(name)
}

fn num(el: Option<&XmlNode>, name: &str) -> Option<f64> {
    number(attr(el, name))
}

// ---- Colors.

type Theme = HashMap<String, String>;

fn scheme_alias(name: &str) -> &str {
    match name {
        "tx1" => "dk1",
        "tx2" => "dk2",
        "bg1" => "lt1",
        "bg2" => "lt2",
        other => other,
    }
}

fn read_theme(root: Option<&XmlNode>) -> Theme {
    let mut theme = Theme::new();
    let Some(scheme) = root.and_then(|r| r.find("clrScheme")) else {
        return theme;
    };
    for entry in &scheme.children {
        let value =
            attr(entry.child("srgbClr"), "val").or_else(|| attr(entry.child("sysClr"), "lastClr"));
        if let Some(value) = value {
            theme.insert(entry.name.clone(), format!("#{value}"));
        }
    }
    theme
}

fn hex_to_rgb(hex: &str) -> [f64; 3] {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    [
        f64::from((value >> 16) & 255),
        f64::from((value >> 8) & 255),
        f64::from(value & 255),
    ]
}

fn rgb_to_hex([r, g, b]: [f64; 3]) -> String {
    let part = |value: f64| value.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", part(r), part(g), part(b))
}

/// lumMod/lumOff/tint/shade, roughly as Office applies them.
fn modulate(hex: &str, el: &XmlNode) -> String {
    let mut rgb = hex_to_rgb(hex);
    for step in &el.children {
        let value = num(Some(step), "val").unwrap_or(100_000.0) / 100_000.0;
        match step.name.as_str() {
            "lumMod" | "shade" => rgb = rgb.map(|c| c * value),
            "lumOff" => rgb = rgb.map(|c| c + 255.0 * value),
            "tint" => rgb = rgb.map(|c| c + (255.0 - c) * (1.0 - value)),
            _ => {}
        }
    }
    rgb_to_hex(rgb)
}

/// The color of a fill-like element (solidFill, a run's color...).
fn color_of(el: Option<&XmlNode>, theme: &Theme) -> Option<String> {
    for entry in &el?.children {
        let hex = match entry.name.as_str() {
            "srgbClr" => Some(format!("#{}", entry.attr("val").unwrap_or(""))),
            "sysClr" => entry.attr("lastClr").map(|v| format!("#{v}")),
            "schemeClr" => theme
                .get(scheme_alias(entry.attr("val").unwrap_or("")))
                .cloned(),
            "prstClr" => Some(if entry.attr("val") == Some("white") {
                "#FFFFFF".to_string()
            } else {
                "#000000".to_string()
            }),
            _ => None,
        };
        if let Some(hex) = hex {
            if hex.len() == 7 && hex[1..].chars().all(|c| c.is_ascii_hexdigit()) {
                return Some(modulate(&hex, entry));
            }
        }
    }
    None
}

fn fill_of(sp_pr: Option<&XmlNode>, theme: &Theme) -> Option<String> {
    color_of(sp_pr.and_then(|s| s.child("solidFill")), theme)
}

// ---- Placeholders and inherited text styles.

struct Placeholder {
    kind: String,
    idx: Option<String>,
}

fn placeholder_of(shape: &XmlNode) -> Option<Placeholder> {
    let nv = shape
        .path(&["nvSpPr", "nvPr"])
        .or_else(|| shape.path(&["nvPicPr", "nvPr"]))
        .or_else(|| shape.path(&["nvGraphicFramePr", "nvPr"]));
    let ph = nv?.child("ph")?;
    Some(Placeholder {
        kind: ph.attr("type").unwrap_or("body").to_string(),
        idx: ph.attr("idx").map(str::to_string),
    })
}

fn find_placeholder<'a>(tree: Option<&'a XmlNode>, wanted: &Placeholder) -> Option<&'a XmlNode> {
    let shapes: Vec<&XmlNode> = tree?.children_named("sp").collect();
    let typed = |ph: &Placeholder| {
        ph.kind == wanted.kind
            || (wanted.kind == "ctrTitle" && ph.kind == "title")
            || (wanted.kind == "subTitle" && ph.kind == "body")
    };
    wanted
        .idx
        .as_ref()
        .and_then(|idx| {
            shapes
                .iter()
                .copied()
                .find(|shape| placeholder_of(shape).is_some_and(|ph| ph.idx.as_ref() == Some(idx)))
        })
        .or_else(|| {
            shapes
                .iter()
                .copied()
                .find(|shape| placeholder_of(shape).is_some_and(|ph| typed(&ph)))
        })
}

/// Where a slide's text and placeholders inherit from.
struct Context {
    theme: Theme,
    layout_tree: Option<Rc<XmlNode>>,
    master_tree: Option<Rc<XmlNode>>,
}

impl Context {
    fn master_styles(&self) -> Option<&XmlNode> {
        self.master_tree.as_deref()?.child("txStyles")
    }

    /// The shapes this one inherits from: its layout's, then its master's.
    fn inherited<'a>(&'a self, shape: &XmlNode) -> Vec<&'a XmlNode> {
        let Some(ph) = placeholder_of(shape) else {
            return Vec::new();
        };
        [&self.layout_tree, &self.master_tree]
            .into_iter()
            .filter_map(|tree| {
                let sp_tree = child_path(tree.as_deref(), &["cSld", "spTree"]);
                find_placeholder(sp_tree, &ph)
            })
            .collect()
    }

    fn master_text_style(&self, ph: Option<&Placeholder>) -> Option<&XmlNode> {
        let styles = self.master_styles();
        match ph {
            None => styles?.child("otherStyle"),
            Some(ph) if ph.kind == "title" || ph.kind == "ctrTitle" => styles?.child("titleStyle"),
            Some(_) => styles?.child("bodyStyle"),
        }
    }
}

fn xfrm_of(shape: &XmlNode) -> Option<&XmlNode> {
    shape
        .path(&["spPr", "xfrm"])
        .or_else(|| shape.path(&["grpSpPr", "xfrm"]))
        .or_else(|| shape.child("xfrm"))
}

fn frame_of(xfrm: Option<&XmlNode>) -> Option<Frame> {
    let xfrm = xfrm?;
    let off = xfrm.child("off")?;
    let ext = xfrm.child("ext")?;
    let rotation = num(Some(xfrm), "rot").filter(|r| *r != 0.0);
    Some(Frame {
        x: num(Some(off), "x").unwrap_or(0.0),
        y: num(Some(off), "y").unwrap_or(0.0),
        w: num(Some(ext), "cx").unwrap_or(0.0),
        h: num(Some(ext), "cy").unwrap_or(0.0),
        rotate: rotation.map(|r| r / 60_000.0),
        flip_h: xfrm.attr("flipH") == Some("1"),
        flip_v: xfrm.attr("flipV") == Some("1"),
    })
}

// ---- Text.

fn align_of(code: &str) -> Align {
    match code {
        "ctr" => Align::Center,
        "r" => Align::Right,
        "just" | "dist" => Align::Justify,
        _ => Align::Left,
    }
}

/// The first value a chain of inheriting elements gives.
fn first<T>(chain: &[Option<&XmlNode>], read: impl Fn(&XmlNode) -> Option<T>) -> Option<T> {
    chain.iter().find_map(|entry| entry.and_then(&read))
}

fn text_body_of(
    body: Option<&XmlNode>,
    lists: &[Option<&XmlNode>],
    body_props: &[Option<&XmlNode>],
    theme: &Theme,
    bullets_by_default: bool,
) -> Option<TextBody> {
    let body = body?;
    let props: Vec<Option<&XmlNode>> = std::iter::once(body.child("bodyPr"))
        .chain(body_props.iter().copied())
        .collect();
    let pick = |name: &str| props.iter().find_map(|entry| attr(*entry, name));
    let autofit = body.child("bodyPr").and_then(|b| b.find("normAutofit"));
    let scale = num(autofit, "fontScale").unwrap_or(100_000.0) / 100_000.0;
    let level_props = |level: u32| -> Vec<Option<&XmlNode>> {
        let name = format!("lvl{}pPr", level + 1);
        std::iter::once(body.child("lstStyle"))
            .chain(lists.iter().copied())
            .map(|list| list.and_then(|l| l.child(&name)))
            .collect()
    };
    let paragraphs = body
        .children_named("p")
        .map(|p| {
            let p_pr = p.child("pPr");
            let level = num(p_pr, "lvl").unwrap_or(0.0).max(0.0) as u32;
            let chain: Vec<Option<&XmlNode>> =
                std::iter::once(p_pr).chain(level_props(level)).collect();
            let default_size = first(&chain, |el| num(el.child("defRPr"), "sz"))
                .unwrap_or(DEFAULT_SIZE * 100.0)
                / 100.0;
            let default_color = first(&chain, |el| {
                color_of(el.path(&["defRPr", "solidFill"]), theme)
            });
            let default_bold = first(&chain, |el| {
                el.child("defRPr")?.attr("b").map(str::to_string)
            });
            let mut runs: Vec<Run> = Vec::new();
            for entry in &p.children {
                if entry.name == "br" {
                    runs.push(Run {
                        text: "\n".into(),
                        size: default_size * scale,
                        bold: false,
                        italic: false,
                        underline: false,
                        color: None,
                    });
                    continue;
                }
                if entry.name != "r" && entry.name != "fld" {
                    continue;
                }
                let text = entry
                    .child("t")
                    .map(|t| t.text_content())
                    .unwrap_or_default();
                if text.is_empty() {
                    continue;
                }
                let r_pr = entry.child("rPr");
                let size = num(r_pr, "sz").unwrap_or(default_size * 100.0) / 100.0;
                let color = color_of(r_pr.and_then(|r| r.child("solidFill")), theme)
                    .or_else(|| default_color.clone());
                let bold = attr(r_pr, "b").or(default_bold.as_deref()) == Some("1");
                runs.push(Run {
                    text,
                    size: size * scale,
                    bold,
                    italic: attr(r_pr, "i") == Some("1"),
                    underline: attr(r_pr, "u").is_some_and(|u| u != "none"),
                    color,
                });
            }
            let bullet_char = first(&chain, |el| {
                if el.child("buNone").is_some() {
                    Some(String::new())
                } else {
                    el.child("buChar")
                        .and_then(|b| b.attr("char"))
                        .map(str::to_string)
                        .or_else(|| el.child("buAutoNum").map(|_| "•".to_string()))
                }
            });
            let bullet = if !runs.is_empty() && bullet_char.as_deref() != Some("") {
                bullet_char.or_else(|| bullets_by_default.then(|| "•".to_string()))
            } else {
                None
            };
            Paragraph {
                runs,
                align: align_of(
                    first(&chain, |el| el.attr("algn").map(str::to_string))
                        .as_deref()
                        .unwrap_or("l"),
                ),
                level,
                bullet,
                size: default_size * scale,
            }
        })
        .collect();
    Some(TextBody {
        paragraphs,
        anchor: match pick("anchor") {
            Some("ctr") => Anchor::Middle,
            Some("b") => Anchor::Bottom,
            _ => Anchor::Top,
        },
        inset: [
            number(pick("tIns")).unwrap_or(45_720.0),
            number(pick("rIns")).unwrap_or(91_440.0),
            number(pick("bIns")).unwrap_or(45_720.0),
            number(pick("lIns")).unwrap_or(91_440.0),
        ],
    })
}

// ---- Shapes.

type Mapping<'a> = &'a dyn Fn(Frame) -> Frame;

fn read_shape(
    shape: &XmlNode,
    deck: &mut Deckage,
    slide_path: &str,
    context: &Context,
    map: Mapping<'_>,
    depth: usize,
) -> Vec<SlideItem> {
    match shape.name.as_str() {
        "sp" => {
            let sources = context.inherited(shape);
            let raw = frame_of(xfrm_of(shape))
                .or_else(|| sources.iter().find_map(|entry| frame_of(xfrm_of(entry))));
            let Some(raw) = raw else {
                return Vec::new();
            };
            let ph = placeholder_of(shape);
            let sp_pr = shape.child("spPr");
            let fill = if sp_pr.is_some_and(|s| s.child("noFill").is_some()) {
                None
            } else {
                fill_of(sp_pr, &context.theme).or_else(|| {
                    sources
                        .iter()
                        .find_map(|entry| fill_of(entry.child("spPr"), &context.theme))
                })
            };
            let line = if child_path(sp_pr, &["ln", "noFill"]).is_some() {
                None
            } else {
                color_of(child_path(sp_pr, &["ln", "solidFill"]), &context.theme)
            };
            let geometry = attr(sp_pr.and_then(|s| s.child("prstGeom")), "prst");
            let mut lists: Vec<Option<&XmlNode>> = sources
                .iter()
                .map(|entry| entry.path(&["txBody", "lstStyle"]))
                .collect();
            lists.push(context.master_text_style(ph.as_ref()));
            let body_props: Vec<Option<&XmlNode>> = sources
                .iter()
                .map(|entry| entry.path(&["txBody", "bodyPr"]))
                .collect();
            let bullets = ph
                .as_ref()
                .is_some_and(|ph| !matches!(ph.kind.as_str(), "title" | "ctrTitle" | "subTitle"));
            let text = text_body_of(
                shape.child("txBody"),
                &lists,
                &body_props,
                &context.theme,
                bullets,
            );
            let has_text = text
                .as_ref()
                .is_some_and(|t| t.paragraphs.iter().any(|p| !p.runs.is_empty()));
            if fill.is_none() && line.is_none() && !has_text {
                return Vec::new();
            }
            vec![SlideItem::Shape(ShapeItem {
                frame: map(raw),
                fill,
                line,
                shape: match geometry {
                    Some("ellipse") => Some(ShapeGeometry::Ellipse),
                    Some("roundRect") => Some(ShapeGeometry::Round),
                    _ => None,
                },
                text: if has_text { text } else { None },
            })]
        }
        "pic" => {
            let raw = frame_of(xfrm_of(shape)).or_else(|| {
                context
                    .inherited(shape)
                    .iter()
                    .find_map(|entry| frame_of(xfrm_of(entry)))
            });
            let embed = shape
                .child("blipFill")
                .and_then(|b| b.find("blip"))
                .and_then(|b| b.rel_attr("embed"));
            let image = embed
                .and_then(|id| deck.target(slide_path, id))
                .and_then(|target| deck.image(&target));
            let Some(raw) = raw else {
                return Vec::new();
            };
            match image {
                Some(image) => vec![SlideItem::Image(ImageItem {
                    frame: map(raw),
                    image,
                })],
                None => vec![SlideItem::Unsupported(UnsupportedItem {
                    frame: map(raw),
                    label: "Picture".into(),
                })],
            }
        }
        "graphicFrame" => {
            let Some(raw) = frame_of(shape.child("xfrm")) else {
                return Vec::new();
            };
            let Some(table) = shape.find("tbl") else {
                let uri = shape
                    .find("graphicData")
                    .and_then(|g| g.attr("uri"))
                    .unwrap_or("");
                let label = if uri.contains("chart") {
                    "Chart"
                } else if uri.contains("diagram") {
                    "Diagram"
                } else {
                    "Object"
                };
                return vec![SlideItem::Unsupported(UnsupportedItem {
                    frame: map(raw),
                    label: label.into(),
                })];
            };
            let frame = map(raw);
            let ratio = if raw.w != 0.0 { frame.w / raw.w } else { 1.0 };
            let columns = table
                .child("tblGrid")
                .map(|grid| {
                    grid.children_named("gridCol")
                        .map(|col| num(Some(col), "w").unwrap_or(0.0) * ratio)
                        .collect()
                })
                .unwrap_or_default();
            let other_style = [context.master_styles().and_then(|s| s.child("otherStyle"))];
            let rows = table
                .children_named("tr")
                .map(|row| TableRow {
                    height: num(Some(row), "h").unwrap_or(0.0) * ratio,
                    cells: row
                        .children_named("tc")
                        .filter(|cell| {
                            cell.attr("hMerge") != Some("1") && cell.attr("vMerge") != Some("1")
                        })
                        .map(|cell| {
                            let fill = fill_of(cell.child("tcPr"), &context.theme);
                            TableCell {
                                span: num(Some(cell), "gridSpan").unwrap_or(1.0).max(1.0) as u32,
                                fill,
                                text: text_body_of(
                                    cell.child("txBody"),
                                    &other_style,
                                    &[],
                                    &context.theme,
                                    false,
                                )
                                .unwrap_or(TextBody {
                                    paragraphs: Vec::new(),
                                    anchor: Anchor::Top,
                                    inset: [45_720.0, 91_440.0, 45_720.0, 91_440.0],
                                }),
                            }
                        })
                        .collect(),
                })
                .collect();
            vec![SlideItem::Table(TableItem {
                frame,
                columns,
                rows,
            })]
        }
        "grpSp" => {
            let xfrm = shape.path(&["grpSpPr", "xfrm"]);
            let outer = frame_of(xfrm);
            let ch_off = xfrm.and_then(|x| x.child("chOff"));
            let ch_ext = xfrm.and_then(|x| x.child("chExt"));
            let (inner_x, inner_y, inner_w, inner_h) = (
                num(ch_off, "x").unwrap_or(0.0),
                num(ch_off, "y").unwrap_or(0.0),
                num(ch_ext, "cx").unwrap_or(0.0),
                num(ch_ext, "cy").unwrap_or(0.0),
            );
            let Some(outer) = outer else {
                return read_tree(
                    Some(shape),
                    deck,
                    slide_path,
                    context,
                    map,
                    false,
                    depth + 1,
                );
            };
            let sx = if inner_w != 0.0 {
                outer.w / inner_w
            } else {
                1.0
            };
            let sy = if inner_h != 0.0 {
                outer.h / inner_h
            } else {
                1.0
            };
            let group_map = move |frame: Frame| {
                map(Frame {
                    x: outer.x + (frame.x - inner_x) * sx,
                    y: outer.y + (frame.y - inner_y) * sy,
                    w: frame.w * sx,
                    h: frame.h * sy,
                    ..frame
                })
            };
            read_tree(
                Some(shape),
                deck,
                slide_path,
                context,
                &group_map,
                false,
                depth + 1,
            )
        }
        // Newer content with a fallback older readers draw.
        "AlternateContent" => read_tree(
            shape.child("Fallback"),
            deck,
            slide_path,
            context,
            map,
            false,
            depth + 1,
        ),
        _ => Vec::new(),
    }
}

fn read_tree(
    tree: Option<&XmlNode>,
    deck: &mut Deckage,
    slide_path: &str,
    context: &Context,
    map: Mapping<'_>,
    // A layout's placeholders hold prompt text ("Click to add title").
    skip_placeholders: bool,
    depth: usize,
) -> Vec<SlideItem> {
    let mut items = Vec::new();
    if depth > MAX_DEPTH {
        return items;
    }
    for shape in tree.map(|t| t.children.as_slice()).unwrap_or_default() {
        if skip_placeholders && placeholder_of(shape).is_some() {
            continue;
        }
        items.extend(read_shape(shape, deck, slide_path, context, map, depth));
    }
    items
}

struct Background {
    color: Option<String>,
    image: Option<ImageData>,
}

fn background_of(roots: &[(Rc<XmlNode>, String)], deck: &mut Deckage, theme: &Theme) -> Background {
    for (root, path) in roots {
        let Some(bg) = root.path(&["cSld", "bg"]) else {
            continue;
        };
        let bg_pr = bg.child("bgPr");
        let color = color_of(bg_pr.and_then(|b| b.child("solidFill")), theme)
            .or_else(|| color_of(bg.child("bgRef"), theme))
            .or_else(|| {
                color_of(
                    bg_pr
                        .and_then(|b| b.child("gradFill"))
                        .and_then(|g| g.find("gs")),
                    theme,
                )
            });
        let embed = bg_pr
            .and_then(|b| b.child("blipFill"))
            .and_then(|b| b.find("blip"))
            .and_then(|b| b.rel_attr("embed"));
        let image = embed
            .and_then(|id| deck.target(path, id))
            .and_then(|target| deck.image(&target));
        if color.is_some() || image.is_some() {
            return Background { color, image };
        }
    }
    Background {
        color: None,
        image: None,
    }
}

/// Reads a .pptx file into its slides, in the order the deck shows them.
pub fn read_pptx(path: impl AsRef<Path>) -> Result<Deck, String> {
    let bytes = std::fs::read(path.as_ref()).map_err(|error| error.to_string())?;
    parse_pptx(bytes)
}

/// Reads a .pptx's bytes into its slides.
pub fn parse_pptx(bytes: Vec<u8>) -> Result<Deck, String> {
    let package = Package::from_bytes(bytes).map_err(|_| NOT_A_DECK.to_string())?;
    let mut deck = Deckage {
        package,
        parts: HashMap::new(),
        rels: HashMap::new(),
    };
    let presentation_path = "ppt/presentation.xml";
    let presentation = deck
        .part(presentation_path)
        .ok_or_else(|| NOT_A_DECK.to_string())?;
    let size = presentation.child("sldSz");
    let width = num(size, "cx").unwrap_or(12_192_000.0);
    let height = num(size, "cy").unwrap_or(6_858_000.0);
    let slide_paths: Vec<String> = presentation
        .child("sldIdLst")
        .map(|list| {
            list.children_named("sldId")
                .filter_map(|entry| deck.target(presentation_path, entry.rel_attr("id")?))
                .take(MAX_SLIDES)
                .collect()
        })
        .unwrap_or_default();
    let mut slides = Vec::new();
    for (index, path) in slide_paths.iter().enumerate() {
        let Some(root) = deck.part(path) else {
            continue;
        };
        if root.attr("show") == Some("0") {
            continue;
        }
        let layout_path = deck.related(path, "slideLayout");
        let layout = layout_path.as_deref().and_then(|p| deck.part(p));
        let master_path = layout_path
            .as_deref()
            .and_then(|p| deck.related(p, "slideMaster"));
        let master = master_path.as_deref().and_then(|p| deck.part(p));
        let theme_path = master_path
            .as_deref()
            .and_then(|p| deck.related(p, "theme"));
        let theme_root = theme_path.as_deref().and_then(|p| deck.part(p));
        let theme = read_theme(theme_root.as_deref());
        let context = Context {
            theme: theme.clone(),
            layout_tree: layout.clone(),
            master_tree: master.clone(),
        };
        let mut roots = vec![(root.clone(), path.clone())];
        if let (Some(layout), Some(layout_path)) = (&layout, &layout_path) {
            roots.push((layout.clone(), layout_path.clone()));
        }
        if let (Some(master), Some(master_path)) = (&master, &master_path) {
            roots.push((master.clone(), master_path.clone()));
        }
        let background = background_of(&roots, &mut deck, &theme);
        // A layout's own pictures and shapes (logos, bands) sit under the
        // slide's, unless the slide says to hide them.
        let from_layout: Vec<SlideItem> = match (&layout, &layout_path) {
            (Some(layout), Some(layout_path)) if root.attr("showMasterSp") != Some("0") => {
                let layout_context = Context {
                    theme: theme.clone(),
                    layout_tree: None,
                    master_tree: master.clone(),
                };
                read_tree(
                    layout.path(&["cSld", "spTree"]),
                    &mut deck,
                    layout_path,
                    &layout_context,
                    &|frame| frame,
                    true,
                    0,
                )
                .into_iter()
                .filter(|item| !matches!(item, SlideItem::Unsupported(_)))
                .collect()
            }
            _ => Vec::new(),
        };
        let own = read_tree(
            root.path(&["cSld", "spTree"]),
            &mut deck,
            path,
            &context,
            &|frame| frame,
            false,
            0,
        );
        slides.push(Slide {
            number: index + 1,
            background: background.color,
            background_image: background.image,
            items: from_layout.into_iter().chain(own).collect(),
        });
    }
    Ok(Deck {
        width,
        height,
        slides,
    })
}

#[cfg(test)]
#[path = "pptx_tests.rs"]
mod tests;
