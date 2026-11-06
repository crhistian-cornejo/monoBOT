//! A PowerPoint deck as its slides, read-only: each slide scaled to fit the
//! panel with its shapes, text, pictures and tables where the file puts them.
//! Ported from the slide half of `DocumentView.tsx`. What the viewer cannot
//! draw (charts, SmartArt, video) shows as a labelled box; rotation and
//! flips are not applied.
use super::office::{self, Pictures, identity};
use super::spreadsheet_chart::parse_hex;
use super::*;
use gpui::{
    Bounds, FontStyle, FontWeight, HighlightStyle, ListAlignment, ListState, Pixels, StyledText,
    UnderlineStyle, canvas, list, relative,
};
use monocode_core::office::pptx::{
    Align, Anchor, Deck, Paragraph, ShapeGeometry, SlideItem, TextBody,
};
use std::{cell::Cell, collections::HashMap, ops::Range, rc::Rc};

/// The gutter around the slides in the panel.
const GUTTER: f32 = 12.;
const EMU_PER_POINT: f32 = 12700.;
/// Half an inch per bullet level, as PowerPoint indents.
const LEVEL_INDENT_EMU: f32 = 457_200.;
const SLIDE_INK: u32 = 0x000000;

pub(super) struct DeckView {
    path: String,
    deck: Arc<Deck>,
    list: ListState,
    pictures: Pictures,
    picture_of: HashMap<usize, usize>,
    width: Rc<Cell<f32>>,
}

/// The size of a paragraph's text: its biggest run, or the paragraph's own.
pub(super) fn paragraph_points(paragraph: &Paragraph) -> f64 {
    paragraph
        .runs
        .iter()
        .map(|run| run.size)
        .fold(None, |best: Option<f64>, size| {
            Some(best.map_or(size, |b| b.max(size)))
        })
        .unwrap_or(paragraph.size)
}

impl DeckView {
    pub(super) fn new(path: &str, deck: Deck, cx: &mut Context<Self>) -> Self {
        let deck = Arc::new(deck);
        let mut picture_of = HashMap::new();
        let mut sources = Vec::new();
        let mut add = |bytes: &Vec<u8>, mime: &str| {
            if let Some(format) = office::format_of(mime, bytes)
                && !picture_of.contains_key(&identity(bytes))
            {
                picture_of.insert(identity(bytes), sources.len());
                sources.push((sources.len(), bytes.clone(), format));
            }
        };
        for slide in &deck.slides {
            if let Some(image) = &slide.background_image {
                add(&image.bytes, &image.mime);
            }
            for item in &slide.items {
                if let SlideItem::Image(image) = item {
                    add(&image.image.bytes, &image.image.mime);
                }
            }
        }
        let pictures = Pictures::new(sources.len());
        office::decode(
            sources,
            |this: &mut DeckView, at, image| this.pictures.set(at, image),
            cx,
        );
        cx.on_release(|this, cx| this.pictures.release(cx)).detach();
        Self {
            path: path.to_string(),
            list: ListState::new(deck.slides.len(), ListAlignment::Top, px(900.)),
            deck,
            pictures,
            picture_of,
            width: Rc::new(Cell::new(0.)),
        }
    }

    pub(super) fn slide_count(&self) -> usize {
        self.deck.slides.len()
    }

    fn picture(&self, bytes: &[u8]) -> Option<Arc<RenderImage>> {
        self.picture_of
            .get(&identity(bytes))
            .and_then(|at| self.pictures.get(*at))
    }

    fn slide_element(&self, index: usize) -> AnyElement {
        let deck = self.deck.clone();
        let Some(slide) = deck.slides.get(index) else {
            return div().into_any_element();
        };
        let width = (self.width.get() - GUTTER * 2.).max(40.);
        // Pixels per EMU.
        let scale = width / deck.width as f32;
        let height = deck.height as f32 * scale;
        let background = slide
            .background
            .as_deref()
            .and_then(parse_hex)
            .unwrap_or(0xffffff);
        let mut surface = div()
            .id(("slide", index))
            .debug_selector(move || format!("slide-{index}"))
            .relative()
            .overflow_hidden()
            .flex_shrink_0()
            .w(px(width))
            .h(px(height))
            .bg(gpui::rgb(background))
            .border_1()
            .border_color(black(0.08));
        if let Some(image) = &slide.background_image
            && let Some(picture) = self.picture(&image.bytes)
        {
            surface = surface.child(
                div().absolute().size_full().child(
                    office::picture(picture)
                        .size_full()
                        .object_fit(ObjectFit::Cover),
                ),
            );
        }
        for (at, item) in slide.items.iter().enumerate() {
            surface = surface.child(
                div()
                    .absolute()
                    .size_full()
                    .debug_selector(move || format!("slide-{index}-item-{at}"))
                    .child(self.item(item, scale)),
            );
        }
        div()
            .w_full()
            .px(px(GUTTER))
            .pt(px(if index == 0 { GUTTER } else { 0. }))
            .pb(px(GUTTER))
            .child(surface)
            .child(
                div()
                    .mt_1()
                    .text_center()
                    .text_size(px(11.))
                    .font_features(crate::right_panel::tabular())
                    .text_color(content(0.4))
                    .child(slide.number.to_string()),
            )
            .into_any_element()
    }

    fn item(&self, item: &SlideItem, scale: f32) -> AnyElement {
        let frame = |f: &monocode_core::office::pptx::Frame| {
            div()
                .absolute()
                .left(px(f.x as f32 * scale))
                .top(px(f.y as f32 * scale))
                .w(px(f.w as f32 * scale))
                .h(px(f.h as f32 * scale))
        };
        match item {
            SlideItem::Image(image) => match self.picture(&image.image.bytes) {
                Some(picture) => frame(&image.frame)
                    .child(
                        office::picture(picture)
                            .size_full()
                            .object_fit(ObjectFit::Fill),
                    )
                    .into_any_element(),
                None => frame(&image.frame).into_any_element(),
            },
            SlideItem::Unsupported(item) => frame(&item.frame)
                .flex()
                .items_center()
                .justify_center()
                .border_1()
                .border_dashed()
                .border_color(black(0.2))
                .bg(black(0.03))
                .text_size(px(11.))
                .text_color(black(0.45))
                .child(item.label.clone())
                .into_any_element(),
            SlideItem::Table(table) => {
                let line = black(0.18);
                let mut grid = div()
                    .absolute()
                    .left(px(table.frame.x as f32 * scale))
                    .top(px(table.frame.y as f32 * scale))
                    .w(px(table.frame.w as f32 * scale))
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_l_1()
                    .border_color(line);
                for row in &table.rows {
                    let mut cells = div().flex().min_h(px(row.height as f32 * scale));
                    let mut column = 0;
                    for cell in &row.cells {
                        let span = cell.span.max(1) as usize;
                        let cell_width: f64 = (column..column + span)
                            .map(|at| table.columns.get(at).copied().unwrap_or(0.))
                            .sum();
                        column += span;
                        let mut holder = div()
                            .w(px(cell_width as f32 * scale))
                            .flex_shrink_0()
                            .border_r_1()
                            .border_b_1()
                            .border_color(line);
                        if let Some(fill) = cell.fill.as_deref().and_then(parse_hex) {
                            holder = holder.bg(gpui::rgb(fill));
                        }
                        cells = cells.child(holder.child(self.text(&cell.text, scale)));
                    }
                    grid = grid.child(cells);
                }
                grid.into_any_element()
            }
            SlideItem::Shape(shape) => {
                let mut surface = frame(&shape.frame).flex().flex_col();
                if let Some(fill) = shape.fill.as_deref().and_then(parse_hex) {
                    surface = surface.bg(gpui::rgb(fill));
                }
                if let Some(line) = shape.line.as_deref().and_then(parse_hex) {
                    surface = surface.border_1().border_color(gpui::rgb(line));
                }
                match shape.shape {
                    Some(ShapeGeometry::Ellipse) => surface = surface.rounded_full(),
                    Some(ShapeGeometry::Round) => {
                        let side = (shape.frame.w.min(shape.frame.h) as f32 * scale) * 0.12;
                        surface = surface.rounded(px(side));
                    }
                    None => {}
                }
                if let Some(body) = &shape.text {
                    surface = surface.child(self.text(body, scale));
                }
                surface.into_any_element()
            }
        }
    }

    fn text(&self, body: &TextBody, scale: f32) -> AnyElement {
        let [top, right, bottom, left] = body.inset.map(|inset| inset as f32 * scale);
        let mut holder = div()
            .flex_1()
            .min_h_full()
            .flex()
            .flex_col()
            .pt(px(top))
            .pr(px(right))
            .pb(px(bottom))
            .pl(px(left))
            .text_color(gpui::rgb(SLIDE_INK))
            .line_height(relative(1.15));
        holder = match body.anchor {
            Anchor::Top => holder.justify_start(),
            Anchor::Middle => holder.justify_center(),
            Anchor::Bottom => holder.justify_end(),
        };
        for paragraph in &body.paragraphs {
            holder = holder.child(self.paragraph(paragraph, scale));
        }
        holder.into_any_element()
    }

    fn paragraph(&self, paragraph: &Paragraph, scale: f32) -> AnyElement {
        let points = |value: f64| value as f32 * EMU_PER_POINT * scale;
        let size = points(paragraph_points(paragraph));
        if paragraph.runs.is_empty() {
            return div()
                .h(px(points(paragraph.size) * 1.15))
                .into_any_element();
        }
        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        for run in &paragraph.runs {
            let start = text.len();
            text.push_str(&run.text);
            if text.len() == start {
                continue;
            }
            let mut style = HighlightStyle {
                color: run
                    .color
                    .as_deref()
                    .and_then(parse_hex)
                    .map(|c| gpui::rgb(c).into()),
                ..HighlightStyle::default()
            };
            if run.bold {
                style.font_weight = Some(FontWeight::BOLD);
            }
            if run.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if run.underline {
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: style.color,
                    wavy: false,
                });
            }
            if style != HighlightStyle::default() {
                highlights.push((start..text.len(), style));
            }
        }
        let styled = StyledText::new(text).with_highlights(highlights);
        let mut line = div()
            .w_full()
            .min_w_0()
            .text_size(px(size))
            .pl(px(paragraph.level as f32 * LEVEL_INDENT_EMU * scale));
        line = match paragraph.align {
            Align::Center => line.text_center(),
            Align::Right => line.text_right(),
            _ => line.text_left(),
        };
        match &paragraph.bullet {
            Some(bullet) => div()
                .flex()
                .w_full()
                .pl(px(paragraph.level as f32 * LEVEL_INDENT_EMU * scale))
                .child(
                    div()
                        .w(px(size))
                        .flex_shrink_0()
                        .text_size(px(size))
                        .child(bullet.clone()),
                )
                .child(div().flex_1().min_w_0().text_size(px(size)).child(styled))
                .into_any_element(),
            None => line.child(styled).into_any_element(),
        }
    }
}

impl gpui::Render for DeckView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let width = self.width.clone();
        let probe = canvas(
            {
                let entity = entity.clone();
                move |bounds: Bounds<Pixels>, _, cx| {
                    let now = bounds.size.width.as_f32();
                    if (width.get() - now).abs() > 0.5 {
                        width.set(now);
                        let entity = entity.clone();
                        cx.defer(move |cx| {
                            entity.update(cx, |this, cx| {
                                this.list.reset(this.deck.slides.len());
                                cx.notify();
                            })
                        });
                    }
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let bar = office::count_bar(&self.path, self.slide_count(), ("slide", "slides"), cx);
        div()
            .id("deck-view")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(bar)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .bg(content(0.05))
                    .child(probe)
                    .child(
                        list(self.list.clone(), move |index, _, cx| {
                            entity.read(cx).slide_element(index)
                        })
                        .size_full(),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::office::pptx::Run;

    #[test]
    fn a_paragraph_is_as_big_as_its_biggest_run() {
        let run = |size: f64| Run {
            text: "x".into(),
            size,
            bold: false,
            italic: false,
            underline: false,
            color: None,
        };
        let paragraph = Paragraph {
            runs: vec![run(12.), run(24.)],
            align: Align::Left,
            level: 0,
            bullet: None,
            size: 18.,
        };
        assert_eq!(paragraph_points(&paragraph), 24.);
        let empty = Paragraph {
            runs: vec![],
            ..paragraph
        };
        assert_eq!(paragraph_points(&empty), 18.);
    }
}
