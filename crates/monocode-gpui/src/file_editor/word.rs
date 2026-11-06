//! A Word document as it reads, read-only: white pages with the document's
//! paragraphs, runs, lists, tables, pictures, header and footer. The Tauri
//! viewer lets docx-preview paginate; the core reads blocks, not pages, so the
//! pages here are filled by an estimate of each block's height.
use super::office::{self, Pictures, identity};
use super::spreadsheet_chart::parse_hex;
use super::*;
use gpui::{
    Bounds, FontStyle, FontWeight, HighlightStyle, InteractiveText, ListAlignment, ListState,
    Pixels, StrikethroughStyle, StyledText, UnderlineStyle, canvas, list, relative,
};
use monocode_core::office::docx::{
    Align, Block, DocxDocument, PageSetup, Paragraph, Run, Table, VerticalAlign,
};
use std::{cell::Cell, collections::HashMap, ops::Range, rc::Rc};

/// The gutter around the pages in the panel.
const GUTTER: f32 = 12.;
const PAPER: u32 = 0xffffff;
const INK: u32 = 0x000000;
const LINK: u32 = 0x0563c1;
const TABLE_LINE: u32 = 0xbfbfbf;
const EMU_PER_POINT: f64 = 12700.;
const BODY_SIZE: f64 = 11.;
const LIST_INDENT: f64 = 18.;
/// Space after a paragraph, in points.
const PARAGRAPH_GAP: f64 = 6.;
/// Letter-sized, one inch of margin: a document that does not say.
const DEFAULT_PAGE: PageSetup = PageSetup {
    width: 612.,
    height: 792.,
    margin_top: 72.,
    margin_right: 72.,
    margin_bottom: 72.,
    margin_left: 72.,
};

pub(super) struct DocumentView {
    path: String,
    document: Arc<DocxDocument>,
    page: PageSetup,
    pages: Vec<Range<usize>>,
    list: ListState,
    pictures: Pictures,
    picture_of: HashMap<usize, usize>,
    width: Rc<Cell<f32>>,
    links: usize,
}

fn heading_size(level: u8) -> f64 {
    match level {
        1 => 20.,
        2 => 16.,
        3 => 14.,
        4 => 12.,
        _ => BODY_SIZE,
    }
}

fn paragraph_size(paragraph: &Paragraph) -> f64 {
    paragraph
        .runs
        .iter()
        .filter(|run| run.image.is_none())
        .filter_map(|run| run.size)
        .fold(None, |best: Option<f64>, size| {
            Some(best.map_or(size, |b| b.max(size)))
        })
        .or(paragraph.heading.map(heading_size))
        .unwrap_or(BODY_SIZE)
}

/// How tall a block will be, in points, when laid in `width` points.
pub(super) fn estimate(block: &Block, width: f64) -> f64 {
    match block {
        Block::Paragraph(paragraph) => {
            let size = paragraph_size(paragraph);
            let indent = paragraph.indent.unwrap_or(0.)
                + paragraph
                    .list
                    .as_ref()
                    .map_or(0., |l| (l.level as f64 + 1.) * LIST_INDENT);
            let room = (width - indent).max(size * 2.);
            let per_line = (room / (size * 0.52)).floor().max(1.);
            let text: String = paragraph
                .runs
                .iter()
                .filter(|run| run.image.is_none())
                .map(|run| run.text.as_str())
                .collect();
            let lines: f64 = text
                .split('\n')
                .map(|line| (line.chars().count() as f64 / per_line).ceil().max(1.))
                .sum();
            let pictures: f64 = paragraph
                .runs
                .iter()
                .filter_map(|run| run.image.as_ref())
                .map(|image| image_height(image.width, image.height, room))
                .sum();
            let text_height = if text.is_empty() && pictures > 0. {
                0.
            } else {
                lines * size * 1.25
            };
            text_height + pictures + PARAGRAPH_GAP
        }
        Block::Table(table) => {
            let total: f64 = table.columns.iter().sum::<f64>().max(1.);
            table
                .rows
                .iter()
                .map(|row| {
                    row.cells
                        .iter()
                        .enumerate()
                        .map(|(index, cell)| {
                            let share = table.columns.get(index).copied().unwrap_or(total / 4.);
                            let inner = (share * f64::from(cell.col_span.max(1)) - 8.).max(24.);
                            cell.blocks.iter().map(|b| estimate(b, inner)).sum::<f64>() + 8.
                        })
                        .fold(0., f64::max)
                })
                .sum::<f64>()
                + PARAGRAPH_GAP
        }
    }
}

/// An inline picture's height in points, scaled down to fit the line.
fn image_height(width: f64, height: f64, room: f64) -> f64 {
    let (w, h) = (width / EMU_PER_POINT, height / EMU_PER_POINT);
    if w <= 0. || h <= 0. {
        return 150.;
    }
    if w > room { h * room / w } else { h }
}

/// Fill pages in order: a block that does not fit starts the next page, a
/// page break before a paragraph does too.
pub(super) fn paginate(document: &DocxDocument, page: &PageSetup) -> Vec<Range<usize>> {
    let width = page.width - page.margin_left - page.margin_right;
    let chrome: f64 = document
        .header
        .iter()
        .chain(&document.footer)
        .map(|block| estimate(block, width))
        .sum();
    let room = (page.height - page.margin_top - page.margin_bottom - chrome).max(72.);
    let mut pages = Vec::new();
    let (mut start, mut used) = (0, 0.);
    for (index, block) in document.blocks.iter().enumerate() {
        let height = estimate(block, width);
        let breaks = matches!(block, Block::Paragraph(p) if p.page_break_before);
        if index > start && (breaks || used + height > room) {
            pages.push(start..index);
            (start, used) = (index, 0.);
        }
        used += height;
    }
    pages.push(start..document.blocks.len());
    pages
}

fn align_of(align: Option<Align>) -> Align {
    align.unwrap_or(Align::Left)
}

impl DocumentView {
    pub(super) fn new(path: &str, document: DocxDocument, cx: &mut Context<Self>) -> Self {
        let page = document.page.unwrap_or(DEFAULT_PAGE);
        let pages = paginate(&document, &page);
        let document = Arc::new(document);
        let mut picture_of = HashMap::new();
        let mut sources = Vec::new();
        let mut visit = |blocks: &[Block]| {
            walk(blocks, &mut |run: &Run| {
                if let Some(image) = &run.image
                    && let Some(format) = office::format_of(&image.mime, &image.bytes)
                    && !picture_of.contains_key(&identity(&image.bytes))
                {
                    picture_of.insert(identity(&image.bytes), sources.len());
                    sources.push((sources.len(), image.bytes.clone(), format));
                }
            });
        };
        visit(&document.header);
        visit(&document.blocks);
        visit(&document.footer);
        let pictures = Pictures::new(sources.len());
        office::decode(
            sources,
            |this: &mut DocumentView, at, image| this.pictures.set(at, image),
            cx,
        );
        cx.on_release(|this, cx| this.pictures.release(cx)).detach();
        Self {
            path: path.to_string(),
            list: ListState::new(pages.len(), ListAlignment::Top, px(900.)),
            document,
            page,
            pages,
            pictures,
            picture_of,
            width: Rc::new(Cell::new(0.)),
            links: 0,
        }
    }

    pub(super) fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Points to pixels: the page keeps its size, scaled down to fit.
    fn scale(&self) -> f32 {
        let room = self.width.get() - GUTTER * 2.;
        let natural = 4. / 3.;
        if room <= 0. {
            return natural;
        }
        (room / self.page.width as f32).min(natural).max(0.05)
    }

    fn page_element(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let s = self.scale();
        let page = self.page;
        let range = self.pages.get(index).cloned().unwrap_or(0..0);
        let document = self.document.clone();
        self.links = 0;
        let mut body = div().flex().flex_col().flex_1();
        let first = range.start;
        for (offset, block) in document.blocks[range].iter().enumerate() {
            let at = first + offset;
            let width = (page.width - page.margin_left - page.margin_right) as f32;
            body = body.child(
                div()
                    .debug_selector(move || format!("docx-block-{at}"))
                    .child(self.block(block, s, width, cx)),
            );
        }
        let header = self.blocks(&document.header, s, page.width as f32, cx);
        let footer = self.blocks(&document.footer, s, page.width as f32, cx);
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(GUTTER))
            .pt(px(if index == 0 { GUTTER } else { 0. }))
            .pb(px(GUTTER))
            .child(
                div()
                    .id(("docx-page", index))
                    .debug_selector(move || format!("docx-page-{index}"))
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .w(px(page.width as f32 * s))
                    .min_h(px(page.height as f32 * s))
                    .pt(px(page.margin_top as f32 * s))
                    .pb(px(page.margin_bottom as f32 * s))
                    .px(px(page.margin_left as f32 * s))
                    .bg(gpui::rgb(PAPER))
                    .text_color(gpui::rgb(INK))
                    .border_1()
                    .border_color(black(0.08))
                    .child(header)
                    .child(body)
                    .child(footer),
            )
            .into_any_element()
    }

    fn blocks(
        &mut self,
        blocks: &[Block],
        s: f32,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut column = div().flex().flex_col();
        for block in blocks {
            column = column.child(self.block(block, s, width, cx));
        }
        column.into_any_element()
    }

    fn block(&mut self, block: &Block, s: f32, width: f32, cx: &mut Context<Self>) -> AnyElement {
        match block {
            Block::Paragraph(paragraph) => self.paragraph(paragraph, s, width, cx),
            Block::Table(table) => self.table(table, s, width, cx),
        }
    }

    fn paragraph(
        &mut self,
        paragraph: &Paragraph,
        s: f32,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let size = paragraph_size(paragraph) as f32 * s;
        let mut text = String::new();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = Vec::new();
        let mut links: Vec<(Range<usize>, String)> = Vec::new();
        let mut rows: Vec<AnyElement> = Vec::new();
        let bold_heading = paragraph.heading.is_some();
        let flush = |this: &mut Self,
                     text: &mut String,
                     highlights: &mut Vec<(Range<usize>, HighlightStyle)>,
                     links: &mut Vec<(Range<usize>, String)>,
                     rows: &mut Vec<AnyElement>,
                     cx: &mut Context<Self>| {
            if text.is_empty() {
                return;
            }
            let styled =
                StyledText::new(std::mem::take(text)).with_highlights(std::mem::take(highlights));
            let line: AnyElement = if links.is_empty() {
                styled.into_any_element()
            } else {
                this.links += 1;
                let ranges: Vec<Range<usize>> = links.iter().map(|(r, _)| r.clone()).collect();
                let targets: Vec<String> = links.drain(..).map(|(_, url)| url).collect();
                InteractiveText::new(("docx-links", this.links), styled)
                    .on_click(ranges, move |index, _, cx| {
                        if let Some(url) = targets.get(index)
                            && (url.starts_with("http://")
                                || url.starts_with("https://")
                                || url.starts_with("mailto:"))
                        {
                            cx.open_url(url);
                        }
                    })
                    .into_any_element()
            };
            let _ = cx;
            rows.push(line);
        };
        for run in &paragraph.runs {
            if let Some(image) = &run.image {
                flush(self, &mut text, &mut highlights, &mut links, &mut rows, cx);
                let natural = image_height(image.width, image.height, width as f64);
                let room = width.max(1.);
                let (w, h) = if image.width > 0. && image.height > 0. {
                    let w = (image.width / EMU_PER_POINT) as f32;
                    let scale = if w > room { room / w } else { 1. };
                    (w * scale, (image.height / EMU_PER_POINT) as f32 * scale)
                } else {
                    (room.min(200.), natural as f32)
                };
                let slot = self.picture_of.get(&identity(&image.bytes)).copied();
                let element = div()
                    .debug_selector(move || format!("docx-image-{}", slot.unwrap_or(usize::MAX)))
                    .w(px(w * s))
                    .h(px(h * s))
                    .flex_shrink_0()
                    .when_some(slot.and_then(|at| self.pictures.get(at)), |d, picture| {
                        d.child(
                            office::picture(picture)
                                .size_full()
                                .object_fit(ObjectFit::Fill),
                        )
                    })
                    .into_any_element();
                rows.push(element);
                continue;
            }
            let start = text.len();
            text.push_str(&run.text.replace('\t', "    "));
            if run.text.is_empty() || text.len() == start {
                continue;
            }
            let range = start..text.len();
            let is_link = run.link.as_ref().is_some_and(|l| !l.starts_with('#'));
            let mut style = HighlightStyle::default();
            let color = if is_link {
                Some(LINK)
            } else {
                run.color.as_deref().and_then(parse_hex)
            };
            style.color = color.map(|c| gpui::rgb(c).into());
            if run.bold || bold_heading {
                style.font_weight = Some(FontWeight::BOLD);
            }
            if run.italic {
                style.font_style = Some(FontStyle::Italic);
            }
            if run.underline || is_link {
                style.underline = Some(UnderlineStyle {
                    thickness: px(1.),
                    color: style.color,
                    wavy: false,
                });
            }
            if run.strike {
                style.strikethrough = Some(StrikethroughStyle {
                    thickness: px(1.),
                    color: style.color,
                });
            }
            style.background_color = run
                .highlight
                .as_deref()
                .and_then(parse_hex)
                .map(|c| gpui::rgb(c).into());
            if matches!(
                run.vertical_align,
                Some(VerticalAlign::Superscript | VerticalAlign::Subscript)
            ) {
                style.fade_out = Some(0.15);
            }
            if style != HighlightStyle::default() {
                highlights.push((range.clone(), style));
            }
            if let (Some(url), true) = (&run.link, is_link) {
                links.push((range, url.clone()));
            }
        }
        flush(self, &mut text, &mut highlights, &mut links, &mut rows, cx);

        let indent = paragraph.indent.unwrap_or(0.) as f32;
        let mut line = div()
            .flex()
            .flex_col()
            .w_full()
            .text_size(px(size))
            .line_height(relative(1.25))
            .pb(px(PARAGRAPH_GAP as f32 * s))
            .pl(px(indent * s));
        line = match align_of(paragraph.align) {
            Align::Center => line.text_center(),
            Align::Right => line.text_right(),
            _ => line.text_left(),
        };
        if rows.is_empty() {
            // An empty paragraph still takes its line.
            rows.push(div().h(px(size * 1.25)).into_any_element());
        }
        let content_column = line.children(rows);
        match &paragraph.list {
            Some(item) => {
                let hang = LIST_INDENT as f32 * s;
                div()
                    .flex()
                    .w_full()
                    .pl(px(item.level as f32 * hang))
                    .child(
                        div()
                            .w(px(hang))
                            .flex_shrink_0()
                            .pr(px(4. * s))
                            .text_size(px(size))
                            .line_height(relative(1.25))
                            .text_right()
                            .child(item.marker.clone()),
                    )
                    .child(div().flex_1().min_w_0().child(content_column))
                    .into_any_element()
            }
            None => content_column.into_any_element(),
        }
    }

    fn table(&mut self, table: &Table, s: f32, width: f32, cx: &mut Context<Self>) -> AnyElement {
        let total: f32 = table.columns.iter().map(|c| *c as f32).sum();
        let fit = if total > width && total > 0. {
            width / total
        } else {
            1.
        };
        let column_width = |index: usize, span: usize| -> f32 {
            (index..index + span)
                .map(|at| table.columns.get(at).copied().unwrap_or(60.) as f32)
                .sum::<f32>()
                * fit
                * s
        };
        let line = gpui::rgb(TABLE_LINE);
        let mut grid = div()
            .flex()
            .flex_col()
            .mb(px(PARAGRAPH_GAP as f32 * s))
            .border_t_1()
            .border_l_1()
            .border_color(line);
        // Columns a cell above spans down into leave a gap in the row below.
        let mut held: Vec<usize> = vec![0; table.columns.len().max(1)];
        for row in &table.rows {
            let mut line_row = div().flex();
            let mut column = 0;
            for cell in &row.cells {
                while column < held.len() && held[column] > 0 {
                    line_row = line_row.child(div().w(px(column_width(column, 1))).flex_shrink_0());
                    held[column] -= 1;
                    column += 1;
                }
                let spans = cell.col_span.max(1) as usize;
                let inner = column_width(column, spans) / s - 8.;
                let mut content_cell = div()
                    .w(px(column_width(column, spans)))
                    .flex_shrink_0()
                    .p(px(4. * s))
                    .border_r_1()
                    .border_b_1()
                    .border_color(line)
                    .overflow_hidden();
                if let Some(fill) = cell.fill.as_deref().and_then(parse_hex) {
                    content_cell = content_cell.bg(gpui::rgb(fill));
                }
                for block in &cell.blocks {
                    content_cell = content_cell.child(self.block(block, s, inner.max(10.), cx));
                }
                line_row = line_row.child(content_cell);
                if cell.row_span > 1 {
                    for at in column..(column + spans).min(held.len()) {
                        held[at] = cell.row_span as usize - 1;
                    }
                }
                column += spans;
            }
            grid = grid.child(line_row);
        }
        grid.into_any_element()
    }
}

/// Every run in the blocks, tables included.
fn walk(blocks: &[Block], visit: &mut dyn FnMut(&Run)) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => paragraph.runs.iter().for_each(&mut *visit),
            Block::Table(table) => {
                for row in &table.rows {
                    for cell in &row.cells {
                        walk(&cell.blocks, visit);
                    }
                }
            }
        }
    }
}

impl gpui::Render for DocumentView {
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
                                this.list.reset(this.pages.len());
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
        let bar = office::count_bar(&self.path, self.page_count(), ("page", "pages"), cx);
        div()
            .id("document-view")
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
                            entity.update(cx, |this, cx| this.page_element(index, cx))
                        })
                        .size_full(),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::office::docx::{Paragraph, Run};

    fn text_block(text: &str) -> Block {
        Block::Paragraph(Paragraph {
            runs: vec![Run {
                text: text.into(),
                ..Run::default()
            }],
            ..Paragraph::default()
        })
    }

    #[test]
    fn long_documents_fill_several_pages_and_breaks_start_one() {
        let long = "palabra ".repeat(60);
        let document = DocxDocument {
            blocks: (0..40).map(|_| text_block(&long)).collect(),
            ..DocxDocument::default()
        };
        let pages = paginate(&document, &DEFAULT_PAGE);
        assert!(pages.len() > 2, "{pages:?}");
        assert_eq!(pages.first().map(|r| r.start), Some(0));
        assert_eq!(pages.last().map(|r| r.end), Some(40));
        for pair in pages.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        let mut broken = DocxDocument {
            blocks: vec![text_block("uno"), text_block("dos")],
            ..DocxDocument::default()
        };
        assert_eq!(paginate(&broken, &DEFAULT_PAGE).len(), 1);
        if let Block::Paragraph(p) = &mut broken.blocks[1] {
            p.page_break_before = true;
        }
        assert_eq!(paginate(&broken, &DEFAULT_PAGE), [0..1, 1..2]);
        assert_eq!(
            paginate(&DocxDocument::default(), &DEFAULT_PAGE).first(),
            Some(&(0..0))
        );
    }

    #[test]
    fn bigger_text_and_pictures_take_more_room() {
        let small = estimate(&text_block("hola"), 400.);
        let mut heading = Paragraph {
            heading: Some(1),
            runs: vec![Run {
                text: "hola".into(),
                ..Run::default()
            }],
            ..Paragraph::default()
        };
        assert!(estimate(&Block::Paragraph(heading.clone()), 400.) > small);
        heading.runs[0].image = Some(monocode_core::office::docx::InlineImage {
            mime: "image/png".into(),
            bytes: vec![],
            width: 100. * EMU_PER_POINT,
            height: 300. * EMU_PER_POINT,
            alt: None,
        });
        assert!(estimate(&Block::Paragraph(heading), 400.) > 300.);
        assert_eq!(image_height(0., 0., 400.), 150.);
        assert_eq!(
            image_height(800. * EMU_PER_POINT, 400. * EMU_PER_POINT, 400.),
            200.
        );
    }
}
