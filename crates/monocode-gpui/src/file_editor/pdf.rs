//! A read-only PDF preview. Windows, macOS and Linux render pages natively off
//! the UI thread, with on-page x/y citation highlights, search and mouse text
//! selection; no document JavaScript or embedded HTML is executed.
use super::*;
use gpui::{
    Bounds, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, canvas, hsla,
    relative,
};
use monocode_core::papers::{PageSpan, extract_page_spans, locate_in_items, merge_span_rects};
use std::{cell::Cell, ops::RangeInclusive, rc::Rc};

fn accent(alpha: f32) -> gpui::Hsla {
    crate::theme::accent_color().opacity(alpha)
}

pub(super) struct PdfEvent(pub String);

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchMatch {
    page: usize,
    snippet: String,
    span_range: Option<RangeInclusive<usize>>,
}

pub(super) struct PdfView {
    path: String,
    bytes: Arc<[u8]>,
    page: usize,
    pages: usize,
    image: Option<Arc<RenderImage>>,
    aspect_ratio: f32,
    spans: Vec<PageSpan>,
    focus_rects: Vec<[f32; 4]>,
    search_rects: Vec<[f32; 4]>,
    selection_rects: Vec<[f32; 4]>,
    selected_spans: Option<RangeInclusive<usize>>,
    drag_anchor: Option<usize>,
    viewport_size: Rc<Cell<(f32, f32)>>,
    /// 1.0 fits the whole page in the view; larger scrolls.
    zoom: f32,
    /// The width the shown bitmap was drawn at.
    drawn_width: f64,
    page_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    error: Option<String>,
    generation: u64,
    _render: Option<Task<()>>,
    text_input: Entity<Input>,
    search_input: Entity<Input>,
    search_matches: Vec<SearchMatch>,
    search_index: usize,
    text: Option<monocode_core::papers::PaperText>,
    text_error: Option<String>,
    text_mode: bool,
    pending_focus: Option<(usize, Option<String>)>,
    _text_task: Option<Task<()>>,
    _text_events: Subscription,
    _search_events: Subscription,
}

impl EventEmitter<PdfEvent> for PdfView {}

impl PdfView {
    pub(super) fn new(path: String, bytes: Vec<u8>, cx: &mut Context<Self>) -> Self {
        cx.on_release(|this, cx| {
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        let text_input = cx.new(|cx| {
            let mut input = Input::editor(cx);
            input.set_read_only(true);
            input
        });
        let search_input = cx.new(|cx| {
            let mut field = Input::field("pdf-search", "Find in PDF…", content(0.35), cx);
            field.set_font_size(12.);
            field
        });
        let text_events = cx.observe(&text_input, |_, _, cx| cx.notify());
        let search_events =
            cx.subscribe(&search_input, |this, _, _: &crate::input::Changed, cx| {
                this.update_search(true, cx);
            });
        let bytes: Arc<[u8]> = bytes.into();
        let initial_spans = page_spans(&bytes, 0);
        let mut view = Self {
            path,
            bytes,
            page: 0,
            pages: 0,
            image: None,
            aspect_ratio: 612.0 / 792.0,
            spans: initial_spans,
            focus_rects: Vec::new(),
            search_rects: Vec::new(),
            selection_rects: Vec::new(),
            selected_spans: None,
            drag_anchor: None,
            viewport_size: Rc::new(Cell::new((0.0, 0.0))),
            zoom: 1.0,
            drawn_width: PAGE_WIDTH,
            page_bounds: Rc::new(Cell::new(None)),
            error: None,
            generation: 0,
            _render: None,
            text_input,
            search_input,
            search_matches: Vec::new(),
            search_index: 0,
            text: None,
            text_error: None,
            text_mode: false,
            pending_focus: None,
            _text_task: None,
            _text_events: text_events,
            _search_events: search_events,
        };
        view.render_page(cx);
        view.load_text(cx);
        view
    }

    fn load_text(&mut self, cx: &mut Context<Self>) {
        let bytes = self.bytes.clone();
        let work = cx
            .background_executor()
            .spawn(async move { extract_text_with_fallback(&bytes) });
        self._text_task = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(text) => {
                        if this.pages == 0 {
                            this.pages = text.total_pages.unwrap_or(text.pages.len());
                        }
                        this.text = Some(text);
                        this.update_text(cx);
                        this.update_search(false, cx);
                        this.apply_focus(cx);
                    }
                    Err(error) => {
                        this.text_error = Some(error);
                        this.apply_focus(cx);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn update_text(&self, cx: &mut Context<Self>) {
        let text = self
            .text
            .as_ref()
            .and_then(|text| text.pages.get(self.page))
            .filter(|text| !text.trim().is_empty())
            .cloned()
            .unwrap_or_else(|| {
                if !self.spans.is_empty() {
                    self.spans
                        .iter()
                        .map(|s| s.text.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                } else {
                    "No readable text is available for this page.".into()
                }
            });
        self.text_input
            .update(cx, |input, cx| input.set_text(text, cx));
    }

    fn selected_quote(&self, cx: &App) -> Option<String> {
        let raw_selected = if !self.text_mode
            && let Some(range) = &self.selected_spans
            && !self.spans.is_empty()
        {
            let start = (*range.start()).min(self.spans.len().saturating_sub(1));
            let end = (*range.end()).min(self.spans.len().saturating_sub(1));
            (start <= end).then(|| {
                self.spans[start..=end]
                    .iter()
                    .map(|span| span.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        } else {
            None
        }
        .or_else(|| {
            let input = self.text_input.read(cx);
            let range = input.selection_range();
            let page = self.text.as_ref()?.pages.get(self.page)?;
            if page.trim().is_empty() || range.is_empty() {
                return None;
            }
            input.content.get(range).map(str::to_owned)
        })?;

        let clean = raw_selected
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if clean.is_empty() {
            return None;
        }
        let truncated = clean.chars().count() > 8_000;
        let mut clean: String = clean.chars().take(8_000).collect();
        if truncated {
            clean.push_str(" [selection truncated]");
        }
        Some(format!(
            "{clean}\n\n- {} (page {})",
            self.path,
            self.page + 1
        ))
    }

    pub(super) fn focus_passage(
        &mut self,
        page: usize,
        passage: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.pending_focus = Some((page.max(1), passage));
        self.apply_focus(cx);
    }

    fn locate_spans_for_passage(&self, passage: &str) -> Option<RangeInclusive<usize>> {
        if self.spans.is_empty() {
            return None;
        }
        let items: Vec<&str> = self.spans.iter().map(|s| s.text.as_str()).collect();
        if let Some(found) = locate_in_items(&items, passage) {
            let end = found.end_item.min(self.spans.len().saturating_sub(1));
            return Some(found.start_item..=end);
        }
        let query_words: Vec<String> = passage
            .split_whitespace()
            .map(|w| {
                w.trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase()
            })
            .filter(|w| !w.is_empty())
            .collect();
        if query_words.is_empty() {
            return None;
        }
        for start in 0..self.spans.len() {
            if start + query_words.len() > self.spans.len() {
                break;
            }
            let matched = query_words.iter().enumerate().all(|(offset, qw)| {
                let sw = self.spans[start + offset]
                    .text
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                sw == *qw || sw.contains(qw)
            });
            if matched {
                return Some(start..=(start + query_words.len() - 1));
            }
        }
        None
    }

    fn apply_focus(&mut self, cx: &mut Context<Self>) {
        let Some((page, passage)) = self.pending_focus.clone() else {
            return;
        };
        if self.pages == 0 {
            return;
        }
        let page = page.min(self.pages).saturating_sub(1);
        if page != self.page {
            self.page = page;
            self.spans = page_spans(&self.bytes, self.page);
            self.focus_rects.clear();
            self.selection_rects.clear();
            self.selected_spans = None;
            self.update_text(cx);
            self.render_page(cx);
        }
        let Some(passage) = passage else {
            self.pending_focus = None;
            return;
        };
        if self.text.is_none() && self.text_error.is_none() && self.spans.is_empty() {
            return;
        }
        let mut matched_on_page = false;
        if let Some(range) = self.locate_spans_for_passage(&passage) {
            self.focus_rects = merge_span_rects(&self.spans[range.clone()]);
            self.selected_spans = Some(range);
            matched_on_page = !self.focus_rects.is_empty();
        }
        if let Some(text) = self.text.as_ref().and_then(|text| text.pages.get(page))
            && let Some(found) = locate_in_items(&[text], &passage)
        {
            let quote = passage.trim();
            let selection = text
                .find(quote)
                .map(|start| start..start + quote.len())
                .unwrap_or(found.start_offset..found.end_offset);
            self.text_input.update(cx, |input, cx| {
                input.select_range(selection, cx);
            });
            if !matched_on_page || self.error.is_some() {
                self.text_mode = true;
            }
        }
        if matched_on_page && self.error.is_none() {
            self.text_mode = false;
        }
        self.pending_focus = None;
        cx.notify();
    }

    fn update_search(&mut self, jump: bool, cx: &mut Context<Self>) {
        let query = self.search_input.read(cx).content.trim().to_owned();
        self.search_matches.clear();
        self.search_rects.clear();
        if query.is_empty() {
            self.search_index = 0;
            cx.notify();
            return;
        }
        let q_lower = query.to_lowercase();
        let page_count = self
            .text
            .as_ref()
            .map(|t| t.pages.len())
            .unwrap_or(self.pages)
            .max(1);
        for p in 0..page_count {
            let p_spans = if p == self.page && !self.spans.is_empty() {
                self.spans.clone()
            } else {
                page_spans(&self.bytes, p)
            };
            let mut found_in_spans = false;
            if !p_spans.is_empty() {
                let q_words: Vec<String> = q_lower
                    .split_whitespace()
                    .map(|w| {
                        w.trim_matches(|c: char| !c.is_alphanumeric())
                            .to_lowercase()
                    })
                    .filter(|w| !w.is_empty())
                    .collect();
                if !q_words.is_empty() {
                    let mut i = 0;
                    while i + q_words.len() <= p_spans.len() {
                        let ok = q_words.iter().enumerate().all(|(k, qw)| {
                            p_spans[i + k].text.to_lowercase().contains(qw.as_str())
                        });
                        if ok {
                            let end = i + q_words.len() - 1;
                            let snippet = p_spans[i..=end]
                                .iter()
                                .map(|s| s.text.as_str())
                                .collect::<Vec<_>>()
                                .join(" ");
                            self.search_matches.push(SearchMatch {
                                page: p,
                                snippet,
                                span_range: Some(i..=end),
                            });
                            found_in_spans = true;
                            i = end + 1;
                        } else {
                            i += 1;
                        }
                    }
                }
            }
            if !found_in_spans
                && let Some(page_text) = self.text.as_ref().and_then(|t| t.pages.get(p))
            {
                let lower = page_text.to_lowercase();
                let mut pos = 0;
                while let Some(rel) = lower[pos..].find(&q_lower) {
                    let start = pos + rel;
                    let end = (start + query.len()).min(page_text.len());
                    let snippet = page_text.get(start..end).unwrap_or(&query).to_owned();
                    self.search_matches.push(SearchMatch {
                        page: p,
                        snippet,
                        span_range: None,
                    });
                    pos = end.max(start + 1);
                    if pos >= lower.len() {
                        break;
                    }
                }
            }
        }
        if self.search_matches.is_empty() {
            self.search_index = 0;
            cx.notify();
            return;
        }
        if jump {
            self.search_index = self
                .search_matches
                .iter()
                .position(|m| m.page >= self.page)
                .unwrap_or(0);
            self.apply_search_match(cx);
        } else {
            self.search_index = self.search_index.min(self.search_matches.len() - 1);
            self.refresh_page_search_rects(cx);
        }
        cx.notify();
    }

    fn step_search(&mut self, next: bool, cx: &mut Context<Self>) {
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len();
        self.search_index = if next {
            (self.search_index + 1) % len
        } else {
            (self.search_index + len - 1) % len
        };
        self.apply_search_match(cx);
    }

    fn apply_search_match(&mut self, cx: &mut Context<Self>) {
        let Some(m) = self.search_matches.get(self.search_index).cloned() else {
            return;
        };
        if m.page != self.page {
            self.page = m.page;
            self.spans = page_spans(&self.bytes, self.page);
            self.update_text(cx);
            self.render_page(cx);
        }
        self.refresh_page_search_rects(cx);
        if let Some(range) = m.span_range {
            let end = (*range.end()).min(self.spans.len().saturating_sub(1));
            if *range.start() <= end && !self.spans.is_empty() {
                self.focus_rects = merge_span_rects(&self.spans[*range.start()..=end]);
                self.selected_spans = Some(*range.start()..=end);
            }
        }
        if let Some(page_text) = self.text.as_ref().and_then(|t| t.pages.get(self.page))
            && let Some(pos) = page_text.to_lowercase().find(&m.snippet.to_lowercase())
        {
            let end = (pos + m.snippet.len()).min(page_text.len());
            self.text_input
                .update(cx, |input, cx| input.select_range(pos..end, cx));
        }
        cx.notify();
    }

    fn refresh_page_search_rects(&mut self, _cx: &mut Context<Self>) {
        self.search_rects.clear();
        for m in &self.search_matches {
            if m.page == self.page
                && let Some(range) = &m.span_range
                && !self.spans.is_empty()
            {
                let start = (*range.start()).min(self.spans.len().saturating_sub(1));
                let end = (*range.end()).min(self.spans.len().saturating_sub(1));
                if start <= end {
                    self.search_rects
                        .extend(merge_span_rects(&self.spans[start..=end]));
                }
            }
        }
    }

    pub(super) fn select_span_range(&mut self, start: usize, end: usize, cx: &mut Context<Self>) {
        if self.spans.is_empty() {
            return;
        }
        let lo = start.min(end).min(self.spans.len() - 1);
        let hi = start.max(end).min(self.spans.len() - 1);
        self.selected_spans = Some(lo..=hi);
        self.selection_rects = merge_span_rects(&self.spans[lo..=hi]);
        cx.notify();
    }

    fn span_at_point(&self, nx: f32, ny: f32) -> Option<usize> {
        if self.spans.is_empty() {
            return None;
        }
        let mut best = None;
        let mut best_dist = f32::MAX;
        for (idx, span) in self.spans.iter().enumerate() {
            let [x, y, w, h] = span.rect;
            let dx = if nx < x {
                x - nx
            } else if nx > x + w {
                nx - (x + w)
            } else {
                0.0
            };
            let dy = if ny < y {
                y - ny
            } else if ny > y + h {
                ny - (y + h)
            } else {
                0.0
            };
            let dist = dx * dx + (dy * 2.5) * (dy * 2.5);
            if dist < best_dist {
                best_dist = dist;
                best = Some(idx);
            }
        }
        (best_dist <= 0.04).then_some(best).flatten()
    }

    fn normalized_page_point(&self, position: Point<Pixels>) -> Option<(f32, f32)> {
        let bounds = self.page_bounds.get()?;
        let w = bounds.size.width.as_f32();
        let h = bounds.size.height.as_f32();
        if w <= 1.0 || h <= 1.0 {
            return None;
        }
        let nx = ((position.x - bounds.origin.x).as_f32() / w).clamp(0.0, 1.0);
        let ny = ((position.y - bounds.origin.y).as_f32() / h).clamp(0.0, 1.0);
        Some((nx, ny))
    }

    pub(super) fn has_bytes(&self, bytes: &[u8]) -> bool {
        self.bytes.as_ref() == bytes
    }

    fn render_page(&mut self, cx: &mut Context<Self>) {
        self.draw_page(false, cx);
    }

    /// `keep` leaves the shown bitmap up until the new one arrives, so a zoom
    /// step does not flash blank.
    fn draw_page(&mut self, keep: bool, cx: &mut Context<Self>) {
        self.generation += 1;
        if !keep && let Some(old) = self.image.take() {
            cx.drop_image(old, None);
        }
        self.error = None;
        let generation = self.generation;
        let page = self.page;
        let bytes = self.bytes.clone();
        let width = self.drawn_width;
        let renderer = cx.svg_renderer();
        let work = cx.background_executor().spawn(async move {
            let spans = page_spans(&bytes, page);
            let (count, png) = render_width(&bytes, page, width)?;
            let ratio = image::ImageReader::new(std::io::Cursor::new(&png))
                .with_guessed_format()
                .ok()
                .and_then(|r| r.into_dimensions().ok())
                .filter(|&(w, h)| w > 0 && h > 0)
                .map(|(w, h)| w as f32 / h as f32)
                .unwrap_or(612.0 / 792.0);
            let image = Image::from_bytes(ImageFormat::Png, png)
                .to_image_data(renderer)
                .map_err(|error| format!("Could not decode the PDF page: {error}"))?;
            Ok::<_, String>((count, image, ratio, spans))
        });
        let fallback_bytes = self.bytes.clone();
        self._render = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                if this.generation != generation {
                    if let Ok((_, image, _, _)) = result {
                        cx.drop_image(image, None);
                    }
                    return;
                }
                match result {
                    Ok((count, image, ratio, spans)) => {
                        if let Some(old) = this.image.replace(image) {
                            cx.drop_image(old, None);
                        }
                        this.pages = count;
                        this.aspect_ratio = ratio;
                        if !spans.is_empty() {
                            this.spans = spans;
                        }
                        this.error = None;
                        this.refresh_page_search_rects(cx);
                        this.apply_focus(cx);
                    }
                    Err(error) => {
                        if let Some(old) = this.image.take() {
                            cx.drop_image(old, None);
                        }
                        let spans = page_spans(&fallback_bytes, page);
                        if !spans.is_empty() {
                            this.spans = spans;
                        }
                        this.error = Some(error);
                        this.apply_focus(cx);
                    }
                }
                cx.notify();
            });
        }));
    }

    fn step_zoom(&mut self, up: bool, cx: &mut Context<Self>) {
        let at = ZOOM_STEPS
            .iter()
            .position(|z| (*z - self.zoom).abs() < 0.01)
            .unwrap_or_else(|| {
                ZOOM_STEPS
                    .iter()
                    .position(|z| *z >= self.zoom)
                    .unwrap_or(ZOOM_STEPS.len() - 1)
            });
        let next = if up {
            (at + 1).min(ZOOM_STEPS.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        self.set_zoom(ZOOM_STEPS[next], cx);
    }

    fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        if (zoom - self.zoom).abs() < 0.001 {
            return;
        }
        self.zoom = zoom;
        // Past a page that fits, the page is drawn wider so it stays sharp.
        let width = (PAGE_WIDTH * f64::from(zoom.max(1.0))).round();
        if (width - self.drawn_width).abs() > 0.5 {
            self.drawn_width = width;
            self.draw_page(true, cx);
        }
        cx.notify();
    }

    fn navigate(&mut self, next: bool, cx: &mut Context<Self>) {
        let page = if next {
            (self.page + 1).min(self.pages.saturating_sub(1))
        } else {
            self.page.saturating_sub(1)
        };
        if page != self.page {
            self.page = page;
            self.spans = page_spans(&self.bytes, self.page);
            self.focus_rects.clear();
            self.selection_rects.clear();
            self.selected_spans = None;
            self.drag_anchor = None;
            self.refresh_page_search_rects(cx);
            self.update_text(cx);
            self.render_page(cx);
            cx.notify();
        }
    }
}

impl Render for PdfView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_quote = self.selected_quote(cx).is_some();
        let search_count = self.search_matches.len();
        let search_active = !self.search_input.read(cx).content.trim().is_empty();
        let entity = cx.entity();
        let viewport_size = self.viewport_size.clone();
        let viewport_probe = canvas(
            {
                let entity = entity.clone();
                move |bounds: Bounds<Pixels>, _, cx| {
                    let now = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
                    let prev = viewport_size.get();
                    if (prev.0 - now.0).abs() > 0.5 || (prev.1 - now.1).abs() > 0.5 {
                        viewport_size.set(now);
                        let entity = entity.clone();
                        cx.defer(move |cx| {
                            entity.update(cx, |_, cx| cx.notify());
                        });
                    }
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let (avail_w, avail_h) = self.viewport_size.get();
        let avail_w = (avail_w - 24.0).max(240.0);
        let avail_h = (avail_h - 24.0).max(320.0);
        let ratio = self.aspect_ratio.clamp(0.2, 5.0);
        let (fit_w, fit_h) = if avail_w / avail_h > ratio {
            (avail_h * ratio, avail_h)
        } else {
            (avail_w, avail_w / ratio)
        };
        let (surface_w, surface_h) = (fit_w * self.zoom, fit_h * self.zoom);
        let zoom_label = format!("{}%", (self.zoom * 100.0).round() as u32);

        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(32.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .border_b_1()
                    .border_color(content(0.1))
                    .text_size(px(12.))
                    .text_color(content(0.6))
                    .child(
                        div()
                            .id("pdf-previous")
                            .role(Role::Button)
                            .aria_label("Previous page")
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child("Previous")
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(false, cx))),
                    )
                    .child(if self.pages == 0 {
                        "PDF".to_owned()
                    } else {
                        format!("Page {} of {}", self.page + 1, self.pages)
                    })
                    .child(
                        div()
                            .id("pdf-next")
                            .role(Role::Button)
                            .aria_label("Next page")
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child("Next")
                            .on_click(cx.listener(|this, _, _, cx| this.navigate(true, cx))),
                    )
                    .child(
                        div()
                            .id("pdf-toggle-text")
                            .role(Role::Button)
                            .aria_label(if self.text_mode {
                                "Show page"
                            } else {
                                "Show text"
                            })
                            .px_2()
                            .rounded_md()
                            .hover(|s| s.bg(content(0.1)))
                            .child(if self.text_mode {
                                "Show page"
                            } else {
                                "Show text"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.text_mode = !this.text_mode;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .w(px(160.))
                            .h(px(22.))
                            .px_2()
                            .rounded_md()
                            .bg(content(0.06))
                            .flex()
                            .items_center()
                            .child(self.search_input.clone()),
                    )
                    .when(search_active, |d| {
                        d.child(
                            div()
                                .text_size(px(11.))
                                .text_color(content(0.45))
                                .child(if search_count == 0 {
                                    "0/0".to_owned()
                                } else {
                                    format!("{}/{search_count}", self.search_index + 1)
                                }),
                        )
                        .when(search_count > 1, |d| {
                            d.child(
                                div()
                                    .id("pdf-search-prev")
                                    .role(Role::Button)
                                    .aria_label("Previous match")
                                    .px_1p5()
                                    .rounded_md()
                                    .hover(|s| s.bg(content(0.1)))
                                    .child("↑")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.step_search(false, cx)),
                                    ),
                            )
                            .child(
                                div()
                                    .id("pdf-search-next")
                                    .role(Role::Button)
                                    .aria_label("Next match")
                                    .px_1p5()
                                    .rounded_md()
                                    .hover(|s| s.bg(content(0.1)))
                                    .child("↓")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.step_search(true, cx)),
                                    ),
                            )
                        })
                    })
                    .when(has_quote, |d| {
                        d.child(
                            div()
                                .id("pdf-add-to-chat")
                                .role(Role::Button)
                                .aria_label("Add selection to chat")
                                .px_2()
                                .rounded_md()
                                .bg(accent(0.16))
                                .text_color(accent_color())
                                .hover(|s| s.bg(accent(0.24)))
                                .child("Add to chat")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(quote) = this.selected_quote(cx) {
                                        cx.emit(PdfEvent(quote));
                                    }
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .p_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(viewport_probe)
                    .when(self.text_mode && self.text.is_some(), |d| {
                        d.child(self.text_input.clone())
                    })
                    .when(self.text_mode && self.text.is_none(), |d| {
                        d.text_size(px(12.)).text_color(content(0.5)).child(
                            self.text_error
                                .clone()
                                .unwrap_or_else(|| "Reading PDF text...".into()),
                        )
                    })
                    .when(
                        !self.text_mode && self.image.is_none() && self.error.is_none(),
                        |d| {
                            d.text_size(px(12.))
                                .text_color(content(0.5))
                                .child("Opening PDF page...")
                        },
                    )
                    .when_some(
                        (!self.text_mode).then(|| self.image.clone()).flatten(),
                        |d, image| {
                            let page_bounds = self.page_bounds.clone();
                            let bounds_probe = canvas(
                                move |bounds: Bounds<Pixels>, _, _| {
                                    page_bounds.set(Some(bounds));
                                },
                                |_, _, _, _| {},
                            )
                            .absolute()
                            .size_full();
                            let focus_rects = self.focus_rects.clone();
                            let search_rects = self.search_rects.clone();
                            let selection_rects = self.selection_rects.clone();
                            let floating_anchor = self
                                .selection_rects
                                .first()
                                .or_else(|| self.focus_rects.first())
                                .copied();
                            d.child(
                                div()
                                    .id("pdf-page-surface")
                                    .debug_selector(|| "pdf-page-surface".into())
                                    .relative()
                                    .w(px(surface_w))
                                    .h(px(surface_h))
                                    .rounded_sm()
                                    .overflow_hidden()
                                    .shadow_sm()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                            if let Some((nx, ny)) =
                                                this.normalized_page_point(event.position)
                                            {
                                                if let Some(idx) = this.span_at_point(nx, ny) {
                                                    this.drag_anchor = Some(idx);
                                                    this.select_span_range(idx, idx, cx);
                                                } else {
                                                    this.drag_anchor = None;
                                                    this.selected_spans = None;
                                                    this.selection_rects.clear();
                                                    cx.notify();
                                                }
                                            }
                                        }),
                                    )
                                    .on_mouse_move(cx.listener(
                                        |this, event: &MouseMoveEvent, _, cx| {
                                            if event.pressed_button == Some(MouseButton::Left)
                                                && let Some(anchor) = this.drag_anchor
                                                && let Some((nx, ny)) =
                                                    this.normalized_page_point(event.position)
                                                && let Some(head) = this.span_at_point(nx, ny)
                                            {
                                                this.select_span_range(anchor, head, cx);
                                            }
                                        },
                                    ))
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|this, _: &MouseUpEvent, _, _| {
                                            this.drag_anchor = None;
                                        }),
                                    )
                                    .child(bounds_probe)
                                    .child(
                                        img(ImageSource::Render(image))
                                            .size_full()
                                            .object_fit(ObjectFit::Fill),
                                    )
                                    .children(search_rects.into_iter().map(|[x, y, w, h]| {
                                        div()
                                            .absolute()
                                            .left(relative(x))
                                            .top(relative(y))
                                            .w(relative(w))
                                            .h(relative(h))
                                            .rounded(px(2.))
                                            .bg(hsla(48. / 360., 0.95, 0.52, 0.22))
                                    }))
                                    .children(focus_rects.into_iter().map(|[x, y, w, h]| {
                                        div()
                                            .debug_selector(|| "pdf-cite-highlight".into())
                                            .absolute()
                                            .left(relative(x))
                                            .top(relative(y))
                                            .w(relative(w))
                                            .h(relative(h))
                                            .rounded(px(2.))
                                            .bg(hsla(45. / 360., 0.95, 0.55, 0.30))
                                            .border_b_2()
                                            .border_color(hsla(38. / 360., 0.92, 0.46, 0.88))
                                    }))
                                    .children(selection_rects.into_iter().map(|[x, y, w, h]| {
                                        div()
                                            .debug_selector(|| "pdf-selection-highlight".into())
                                            .absolute()
                                            .left(relative(x))
                                            .top(relative(y))
                                            .w(relative(w))
                                            .h(relative(h))
                                            .rounded(px(2.))
                                            .bg(accent(0.28))
                                    }))
                                    .when_some(
                                        floating_anchor.filter(|_| has_quote),
                                        |d, [x, y, _, h]| {
                                            let pill_top = if y > 0.08 {
                                                (y - 0.045).max(0.01)
                                            } else {
                                                (y + h + 0.01).min(0.92)
                                            };
                                            d.child(
                                                div()
                                                    .id("pdf-floating-add-to-chat")
                                                    .role(Role::Button)
                                                    .aria_label("Ask in chat about selection")
                                                    .absolute()
                                                    .left(relative(x.clamp(0.02, 0.72)))
                                                    .top(relative(pill_top))
                                                    .px_2p5()
                                                    .py_1()
                                                    .rounded_md()
                                                    .bg(shell_fill())
                                                    .border_1()
                                                    .border_color(content(0.16))
                                                    .shadow_md()
                                                    .text_size(px(11.))
                                                    .text_color(content(0.9))
                                                    .hover(|s| s.bg(content(0.12)))
                                                    .child("Add to chat")
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        |_, _, cx| cx.stop_propagation(),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        if let Some(quote) = this.selected_quote(cx)
                                                        {
                                                            cx.emit(PdfEvent(quote));
                                                        }
                                                    })),
                                            )
                                        },
                                    ),
                            )
                        },
                    )
                    .when_some(
                        (!self.text_mode).then(|| self.error.clone()).flatten(),
                        |d, error| {
                            d.flex_col()
                                .gap_2()
                                .text_size(px(12.))
                                .text_color(content(0.6))
                                .child(error)
                                .when(!self.path.starts_with("remote://"), |d| {
                                    d.child(
                                        div()
                                            .id("pdf-open-externally")
                                            .role(Role::Button)
                                            .aria_label("Open PDF externally")
                                            .px_3()
                                            .py_1()
                                            .rounded_md()
                                            .bg(content(0.1))
                                            .child("Open externally")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                let path = PathBuf::from(&this.path);
                                                cx.background_executor()
                                                    .spawn(async move {
                                                        let _ = monocode_core::files::open_path_with_default_app(&path);
                                                    })
                                                    .detach();
                                            })),
                                    )
                                })
                        },
                    ),
            )
    }
}

fn extract_text_with_fallback(bytes: &[u8]) -> Result<monocode_core::papers::PaperText, String> {
    let mut extracted = monocode_core::papers::extract_pdf_text(bytes);
    #[cfg(target_os = "macos")]
    {
        if let Ok(ref mut paper) = extracted {
            if !paper.unreadable_pages.is_empty()
                && let Some(native_pages) = macos::extract_pages_text(bytes)
            {
                for (idx, page_str) in native_pages.into_iter().enumerate() {
                    if let Some(slot) = paper.pages.get_mut(idx)
                        && slot.trim().is_empty()
                        && !page_str.trim().is_empty()
                    {
                        *slot = page_str;
                        paper.unreadable_pages.retain(|&p| p != idx + 1);
                    }
                }
            }
        } else if let Some(native_pages) = macos::extract_pages_text(bytes)
            && !native_pages.is_empty()
        {
            return Ok(monocode_core::papers::PaperText {
                pages: native_pages,
                unreadable_pages: Vec::new(),
                total_pages: None,
            });
        }
    }
    extracted
}

/// The width a page is drawn at when it is shown to fit.
pub(crate) const PAGE_WIDTH: f64 = 1400.0;
const ZOOM_STEPS: [f32; 8] = [0.5, 0.75, 1.0, 1.25, 1.5, 2.0, 3.0, 4.0];

pub(crate) fn render(bytes: &[u8], page_index: usize) -> Result<(usize, Vec<u8>), String> {
    render_width(bytes, page_index, PAGE_WIDTH)
}

pub(crate) fn page_spans(bytes: &[u8], page_index: usize) -> Vec<PageSpan> {
    #[cfg(target_os = "macos")]
    {
        let native = macos::page_spans(bytes, page_index);
        if !native.is_empty() {
            return native;
        }
    }
    extract_page_spans(bytes, page_index)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub(crate) fn render_width(_: &[u8], _: usize, _: f64) -> Result<(usize, Vec<u8>), String> {
    Err("PDF preview is not available on this platform yet.".into())
}

#[cfg(target_os = "linux")]
pub(crate) fn render_width(
    bytes: &[u8],
    page_index: usize,
    target_width: f64,
) -> Result<(usize, Vec<u8>), String> {
    linux::render(bytes, page_index, target_width)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::ffi::{CString, c_char, c_double, c_int, c_uchar, c_void};

    pub fn render(
        bytes: &[u8],
        page_index: usize,
        target_width: f64,
    ) -> Result<(usize, Vec<u8>), String> {
        if bytes.is_empty() {
            return Err("Could not open this PDF: empty file.".into());
        }
        unsafe {
            let poppler = dlopen(b"libpoppler-glib.so.8\0");
            let cairo = dlopen(b"libcairo.so.2\0");
            let glib = dlopen(b"libglib-2.0.so.0\0");
            let gobject = dlopen(b"libgobject-2.0.so.0\0");
            if poppler.is_null() || cairo.is_null() || glib.is_null() || gobject.is_null() {
                return Err("PDF preview is not available on this platform yet.".into());
            }

            type GBytesNew = unsafe extern "C" fn(*const c_void, usize) -> *mut c_void;
            type GBytesUnref = unsafe extern "C" fn(*mut c_void);
            type GObjectUnref = unsafe extern "C" fn(*mut c_void);
            type PopplerNewFromBytes =
                unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_void) -> *mut c_void;
            type PopplerGetNPages = unsafe extern "C" fn(*mut c_void) -> c_int;
            type PopplerGetPage = unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_void;
            type PopplerPageGetSize =
                unsafe extern "C" fn(*mut c_void, *mut c_double, *mut c_double);
            type PopplerPageRender = unsafe extern "C" fn(*mut c_void, *mut c_void);
            type CairoSurfaceCreate = unsafe extern "C" fn(c_int, c_int, c_int) -> *mut c_void;
            type CairoSurfaceDestroy = unsafe extern "C" fn(*mut c_void);
            type CairoSurfaceFlush = unsafe extern "C" fn(*mut c_void);
            type CairoSurfaceGetData = unsafe extern "C" fn(*mut c_void) -> *mut c_uchar;
            type CairoSurfaceGetStride = unsafe extern "C" fn(*mut c_void) -> c_int;
            type CairoCreate = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
            type CairoDestroy = unsafe extern "C" fn(*mut c_void);
            type CairoScale = unsafe extern "C" fn(*mut c_void, c_double, c_double);
            type CairoSetSourceRgb =
                unsafe extern "C" fn(*mut c_void, c_double, c_double, c_double);
            type CairoPaint = unsafe extern "C" fn(*mut c_void);

            let g_bytes_new: GBytesNew = sym(glib, b"g_bytes_new\0")?;
            let g_bytes_unref: GBytesUnref = sym(glib, b"g_bytes_unref\0")?;
            let g_object_unref: GObjectUnref = sym(gobject, b"g_object_unref\0")?;
            let poppler_new: PopplerNewFromBytes =
                sym(poppler, b"poppler_document_new_from_bytes\0")?;
            let poppler_n_pages: PopplerGetNPages =
                sym(poppler, b"poppler_document_get_n_pages\0")?;
            let poppler_get_page: PopplerGetPage = sym(poppler, b"poppler_document_get_page\0")?;
            let poppler_page_size: PopplerPageGetSize = sym(poppler, b"poppler_page_get_size\0")?;
            let poppler_page_render: PopplerPageRender = sym(poppler, b"poppler_page_render\0")?;
            let cairo_surface_create: CairoSurfaceCreate =
                sym(cairo, b"cairo_image_surface_create\0")?;
            let cairo_surface_destroy: CairoSurfaceDestroy =
                sym(cairo, b"cairo_surface_destroy\0")?;
            let cairo_surface_flush: CairoSurfaceFlush = sym(cairo, b"cairo_surface_flush\0")?;
            let cairo_surface_data: CairoSurfaceGetData =
                sym(cairo, b"cairo_image_surface_get_data\0")?;
            let cairo_surface_stride: CairoSurfaceGetStride =
                sym(cairo, b"cairo_image_surface_get_stride\0")?;
            let cairo_create: CairoCreate = sym(cairo, b"cairo_create\0")?;
            let cairo_destroy: CairoDestroy = sym(cairo, b"cairo_destroy\0")?;
            let cairo_scale: CairoScale = sym(cairo, b"cairo_scale\0")?;
            let cairo_set_source_rgb: CairoSetSourceRgb = sym(cairo, b"cairo_set_source_rgb\0")?;
            let cairo_paint: CairoPaint = sym(cairo, b"cairo_paint\0")?;

            let gbytes = g_bytes_new(bytes.as_ptr() as *const c_void, bytes.len());
            if gbytes.is_null() {
                return Err("Could not allocate PDF buffer.".into());
            }
            let doc = poppler_new(gbytes, std::ptr::null(), std::ptr::null_mut());
            g_bytes_unref(gbytes);
            if doc.is_null() {
                return Err("Could not open this PDF.".into());
            }
            let pages = poppler_n_pages(doc) as usize;
            if pages == 0 || page_index >= pages {
                g_object_unref(doc);
                return Err(format!("Page {page_index} out of range."));
            }
            let page = poppler_get_page(doc, page_index as c_int);
            if page.is_null() {
                g_object_unref(doc);
                return Err("Could not load PDF page.".into());
            }
            let mut pw = 0.0_f64;
            let mut ph = 0.0_f64;
            poppler_page_size(page, &mut pw, &mut ph);
            let scale = (target_width / pw.max(1.0)).min(4096.0 / ph.max(1.0));
            let width = (pw * scale).round().max(1.0) as c_int;
            let height = (ph * scale).round().max(1.0) as c_int;

            // CAIRO_FORMAT_ARGB32 = 0
            let surface = cairo_surface_create(0, width, height);
            let cr = cairo_create(surface);
            cairo_set_source_rgb(cr, 1.0, 1.0, 1.0);
            cairo_paint(cr);
            cairo_scale(cr, scale, scale);
            poppler_page_render(page, cr);
            cairo_surface_flush(surface);

            let data = cairo_surface_data(surface);
            let stride = cairo_surface_stride(surface) as usize;
            let w = width as usize;
            let h = height as usize;
            let mut rgba = vec![0u8; w * h * 4];
            if !data.is_null() {
                let raw = std::slice::from_raw_parts(data, stride * h);
                for y in 0..h {
                    let row = &raw[y * stride..y * stride + w * 4];
                    let dst = &mut rgba[y * w * 4..(y + 1) * w * 4];
                    for x in 0..w {
                        let b = row[x * 4];
                        let g = row[x * 4 + 1];
                        let r = row[x * 4 + 2];
                        let a = row[x * 4 + 3];
                        dst[x * 4] = r;
                        dst[x * 4 + 1] = g;
                        dst[x * 4 + 2] = b;
                        dst[x * 4 + 3] = a;
                    }
                }
            }
            cairo_destroy(cr);
            cairo_surface_destroy(surface);
            g_object_unref(page);
            g_object_unref(doc);

            let mut png = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new(&mut png);
            image::ImageEncoder::write_image(
                encoder,
                &rgba,
                width as u32,
                height as u32,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| format!("Could not encode PDF page: {e}"))?;
            Ok((pages, png))
        }
    }

    unsafe fn dlopen(name: &[u8]) -> *mut c_void {
        unsafe { libc::dlopen(name.as_ptr() as *const c_char, libc::RTLD_LAZY) }
    }

    unsafe fn sym<T: Copy>(handle: *mut c_void, name: &[u8]) -> Result<T, String> {
        let ptr = unsafe { libc::dlsym(handle, name.as_ptr() as *const c_char) };
        if ptr.is_null() {
            let n = CString::from_vec_with_nul(name.to_vec())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            return Err(format!("Missing symbol {n}"));
        }
        Ok(unsafe { std::mem::transmute_copy(&ptr) })
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn render_width(
    bytes: &[u8],
    page_index: usize,
    target_width: f64,
) -> Result<(usize, Vec<u8>), String> {
    macos::render(bytes, page_index, target_width)
}

#[cfg(target_os = "macos")]
mod macos {
    use monocode_core::papers::PageSpan;
    use objc2::msg_send;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSData, NSRange, NSRect, NSString};
    use std::ffi::c_void;

    #[link(name = "PDFKit", kind = "framework")]
    unsafe extern "C" {}

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug)]
    pub struct CGAffineTransform {
        pub a: f64,
        pub b: f64,
        pub c: f64,
        pub d: f64,
        pub tx: f64,
        pub ty: f64,
    }

    pub type CGDataProviderRef = *mut c_void;
    pub type CGPDFDocumentRef = *mut c_void;
    pub type CGPDFPageRef = *mut c_void;
    pub type CGColorSpaceRef = *mut c_void;
    pub type CGContextRef = *mut c_void;

    pub const K_CGPDF_MEDIA_BOX: i32 = 0;
    pub const K_CGPDF_CROP_BOX: i32 = 1;

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        pub fn CGDataProviderCreateWithData(
            info: *mut c_void,
            data: *const c_void,
            size: usize,
            release_data: Option<unsafe extern "C" fn(*mut c_void, *const c_void, usize)>,
        ) -> CGDataProviderRef;
        pub fn CGDataProviderRelease(provider: CGDataProviderRef);

        pub fn CGPDFDocumentCreateWithProvider(provider: CGDataProviderRef) -> CGPDFDocumentRef;
        pub fn CGPDFDocumentRelease(document: CGPDFDocumentRef);
        pub fn CGPDFDocumentGetNumberOfPages(document: CGPDFDocumentRef) -> usize;
        pub fn CGPDFDocumentGetPage(document: CGPDFDocumentRef, page_number: usize)
        -> CGPDFPageRef;

        pub fn CGPDFPageGetBoxRect(page: CGPDFPageRef, box_type: i32) -> CGRect;
        pub fn CGPDFPageGetRotationAngle(page: CGPDFPageRef) -> i32;
        pub fn CGPDFPageGetDrawingTransform(
            page: CGPDFPageRef,
            box_type: i32,
            rect: CGRect,
            rotate: i32,
            preserve_aspect_ratio: bool,
        ) -> CGAffineTransform;

        pub fn CGColorSpaceCreateDeviceRGB() -> CGColorSpaceRef;
        pub fn CGColorSpaceRelease(space: CGColorSpaceRef);

        pub fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: CGColorSpaceRef,
            bitmap_info: u32,
        ) -> CGContextRef;
        pub fn CGContextRelease(context: CGContextRef);

        pub fn CGContextSetRGBFillColor(
            context: CGContextRef,
            red: f64,
            green: f64,
            blue: f64,
            alpha: f64,
        );
        pub fn CGContextFillRect(context: CGContextRef, rect: CGRect);
        pub fn CGContextConcatCTM(context: CGContextRef, transform: CGAffineTransform);
        pub fn CGContextScaleCTM(context: CGContextRef, sx: f64, sy: f64);
        pub fn CGContextDrawPDFPage(context: CGContextRef, page: CGPDFPageRef);
    }

    struct ProviderGuard(CGDataProviderRef);
    impl Drop for ProviderGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CGDataProviderRelease(self.0) };
            }
        }
    }

    struct DocumentGuard(CGPDFDocumentRef);
    impl Drop for DocumentGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CGPDFDocumentRelease(self.0) };
            }
        }
    }

    struct ColorSpaceGuard(CGColorSpaceRef);
    impl Drop for ColorSpaceGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CGColorSpaceRelease(self.0) };
            }
        }
    }

    struct ContextGuard(CGContextRef);
    impl Drop for ContextGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CGContextRelease(self.0) };
            }
        }
    }

    struct ObjGuard(*mut AnyObject);
    impl Drop for ObjGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                let _: () = unsafe { msg_send![self.0, release] };
            }
        }
    }

    pub fn extract_pages_text(bytes: &[u8]) -> Option<Vec<String>> {
        if bytes.is_empty() || !bytes.starts_with(b"%PDF-") {
            return None;
        }
        objc2::rc::autoreleasepool(|_| unsafe {
            let cls = AnyClass::get(c"PDFDocument")?;
            let data = NSData::with_bytes(bytes);
            let doc: *mut AnyObject = msg_send![cls, alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithData: &*data];
            if doc.is_null() {
                return None;
            }
            let _doc_guard = ObjGuard(doc);
            let count: usize = msg_send![doc, pageCount];
            if count == 0 {
                return None;
            }
            let mut out = Vec::with_capacity(count.min(80));
            for i in 0..count.min(80) {
                let page: *mut AnyObject = msg_send![doc, pageAtIndex: i];
                if page.is_null() {
                    out.push(String::new());
                    continue;
                }
                let ns_str: *const NSString = msg_send![page, string];
                if ns_str.is_null() {
                    out.push(String::new());
                } else {
                    out.push((*ns_str).to_string().trim().to_owned());
                }
            }
            Some(out)
        })
    }

    pub fn page_spans(bytes: &[u8], page_index: usize) -> Vec<PageSpan> {
        if bytes.is_empty() || !bytes.starts_with(b"%PDF-") {
            return Vec::new();
        }
        objc2::rc::autoreleasepool(|_| unsafe {
            let Some(cls) = AnyClass::get(c"PDFDocument") else {
                return Vec::new();
            };
            let data = NSData::with_bytes(bytes);
            let doc: *mut AnyObject = msg_send![cls, alloc];
            let doc: *mut AnyObject = msg_send![doc, initWithData: &*data];
            if doc.is_null() {
                return Vec::new();
            }
            let _doc_guard = ObjGuard(doc);
            let count: usize = msg_send![doc, pageCount];
            if page_index >= count {
                return Vec::new();
            }
            let page: *mut AnyObject = msg_send![doc, pageAtIndex: page_index];
            if page.is_null() {
                return Vec::new();
            }
            let mut box_rect: NSRect = msg_send![page, boundsForBox: 1isize];
            if box_rect.size.width <= 0.0 || box_rect.size.height <= 0.0 {
                box_rect = msg_send![page, boundsForBox: 0isize];
            }
            if box_rect.size.width <= 0.0 || box_rect.size.height <= 0.0 {
                return Vec::new();
            }
            let ns_str: *const NSString = msg_send![page, string];
            if ns_str.is_null() {
                return Vec::new();
            }
            let full_text = (*ns_str).to_string();
            let mut spans = Vec::new();
            let mut utf16_pos = 0usize;
            let mut word = String::new();
            let mut word_u16_start = 0usize;
            let mut word_u16_len = 0usize;

            let mut flush = |word: &mut String, start: usize, len: usize| {
                if word.is_empty() || len == 0 {
                    return;
                }
                let text = std::mem::take(word);
                let range = NSRange::new(start, len);
                let sel: *mut AnyObject = msg_send![page, selectionForRange: range];
                if !sel.is_null() {
                    let b: NSRect = msg_send![sel, boundsForPage: page];
                    if b.size.width > 0.0 && b.size.height > 0.0 {
                        let nx = ((b.origin.x - box_rect.origin.x) / box_rect.size.width)
                            .clamp(0.0, 1.0) as f32;
                        let ny = (1.0
                            - (b.origin.y - box_rect.origin.y + b.size.height)
                                / box_rect.size.height)
                            .clamp(0.0, 1.0) as f32;
                        let nw = (b.size.width / box_rect.size.width)
                            .clamp(0.002, (1.0 - nx as f64).max(0.002))
                            as f32;
                        let nh = (b.size.height / box_rect.size.height)
                            .clamp(0.005, (1.0 - ny as f64).max(0.005))
                            as f32;
                        spans.push(PageSpan {
                            text,
                            rect: [nx, ny, nw, nh],
                        });
                    }
                }
            };

            for ch in full_text.chars() {
                let u16_len = ch.len_utf16();
                if ch.is_whitespace() {
                    flush(&mut word, word_u16_start, word_u16_len);
                    word_u16_len = 0;
                } else {
                    if word.is_empty() {
                        word_u16_start = utf16_pos;
                        word_u16_len = 0;
                    }
                    word.push(ch);
                    word_u16_len += u16_len;
                }
                utf16_pos += u16_len;
            }
            flush(&mut word, word_u16_start, word_u16_len);
            spans
        })
    }

    pub fn render(
        bytes: &[u8],
        page_index: usize,
        target_width: f64,
    ) -> Result<(usize, Vec<u8>), String> {
        if bytes.is_empty() {
            return Err("Could not open this PDF: empty file".into());
        }
        let provider = unsafe {
            CGDataProviderCreateWithData(
                std::ptr::null_mut(),
                bytes.as_ptr() as *const c_void,
                bytes.len(),
                None,
            )
        };
        if provider.is_null() {
            return Err("Could not allocate PDF data provider.".into());
        }
        let _provider_guard = ProviderGuard(provider);

        let document = unsafe { CGPDFDocumentCreateWithProvider(provider) };
        if document.is_null() {
            return Err("Could not open this PDF: invalid format.".into());
        }
        let _document_guard = DocumentGuard(document);

        let pages = unsafe { CGPDFDocumentGetNumberOfPages(document) };
        if pages == 0 {
            return Err("Could not open this PDF: no pages found.".into());
        }
        if page_index >= pages {
            return Err(format!(
                "Page index {page_index} out of range (document has {pages} pages)"
            ));
        }

        let page = unsafe { CGPDFDocumentGetPage(document, page_index + 1) };
        if page.is_null() {
            return Err(format!("Could not load PDF page {}.", page_index + 1));
        }

        let mut box_rect = unsafe { CGPDFPageGetBoxRect(page, K_CGPDF_CROP_BOX) };
        if box_rect.size.width <= 0.0 || box_rect.size.height <= 0.0 {
            box_rect = unsafe { CGPDFPageGetBoxRect(page, K_CGPDF_MEDIA_BOX) };
        }
        if box_rect.size.width <= 0.0 || box_rect.size.height <= 0.0 {
            return Err("Could not read PDF page bounds.".into());
        }

        let rotation = unsafe { CGPDFPageGetRotationAngle(page) };
        let (page_w, page_h) = if rotation == 90 || rotation == 270 {
            (box_rect.size.height, box_rect.size.width)
        } else {
            (box_rect.size.width, box_rect.size.height)
        };

        let scale = (target_width / page_w.max(1.0)).min(4096.0 / page_h.max(1.0));
        let width = (page_w * scale).round().max(1.0) as usize;
        let height = (page_h * scale).round().max(1.0) as usize;

        let color_space = unsafe { CGColorSpaceCreateDeviceRGB() };
        if color_space.is_null() {
            return Err("Could not create device color space.".into());
        }
        let _cs_guard = ColorSpaceGuard(color_space);

        let bytes_per_row = width * 4;
        let mut raw_pixels = vec![0u8; bytes_per_row * height];
        // kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big = 1 | 0x4000 = 0x4001
        let context = unsafe {
            CGBitmapContextCreate(
                raw_pixels.as_mut_ptr() as *mut c_void,
                width,
                height,
                8,
                bytes_per_row,
                color_space,
                0x4001,
            )
        };
        if context.is_null() {
            return Err("Could not create bitmap context.".into());
        }
        let _context_guard = ContextGuard(context);

        // Fill background with white
        unsafe {
            CGContextSetRGBFillColor(context, 1.0, 1.0, 1.0, 1.0);
            CGContextFillRect(
                context,
                CGRect {
                    origin: CGPoint { x: 0.0, y: 0.0 },
                    size: CGSize {
                        width: width as f64,
                        height: height as f64,
                    },
                },
            );
        }

        // The drawing transform only ever shrinks a page to fit its target and
        // never enlarges it, so a page asked to fill a bigger bitmap came out
        // small and centred. Scale the bitmap up here and let the transform map
        // the page onto a rectangle of exactly its own size.
        let scale_x = width as f64 / page_w;
        let scale_y = height as f64 / page_h;
        let target_rect = CGRect {
            origin: CGPoint { x: 0.0, y: 0.0 },
            size: CGSize {
                width: page_w,
                height: page_h,
            },
        };
        let transform =
            unsafe { CGPDFPageGetDrawingTransform(page, K_CGPDF_CROP_BOX, target_rect, 0, true) };
        unsafe {
            CGContextScaleCTM(context, scale_x, scale_y);
            CGContextConcatCTM(context, transform);
            CGContextDrawPDFPage(context, page);
        }

        let mut png = Vec::new();
        let encoder = image::codecs::png::PngEncoder::new(&mut png);
        image::ImageEncoder::write_image(
            encoder,
            &raw_pixels,
            width as u32,
            height as u32,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("Could not encode PDF page to PNG: {e}"))?;

        Ok((pages, png))
    }
}

#[cfg(target_os = "windows")]
pub(crate) fn render_width(
    bytes: &[u8],
    page_index: usize,
    target_width: f64,
) -> Result<(usize, Vec<u8>), String> {
    use windows::{
        Data::Pdf::{PdfDocument, PdfPageRenderOptions},
        Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream},
        Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize},
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }
    unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|error| error.to_string())?;
    let _apartment = Apartment;
    let draw = || -> windows::core::Result<(usize, Vec<u8>)> {
        let input = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&input)?;
        writer.WriteBytes(bytes)?;
        writer.StoreAsync()?.join()?;
        writer.DetachStream()?;
        input.Seek(0)?;
        let document = PdfDocument::LoadFromStreamAsync(&input)?.join()?;
        let pages = document.PageCount()? as usize;
        if page_index >= pages {
            return Err(windows::core::Error::from(
                windows::Win32::Foundation::E_INVALIDARG,
            ));
        }
        let page = document.GetPage(page_index as u32)?;
        let size = page.Size()?;
        let scale = (target_width as f32 / size.Width.max(1.0)).min(4096.0 / size.Height.max(1.0));
        let options = PdfPageRenderOptions::new()?;
        options.SetDestinationWidth((size.Width * scale).round().max(1.0) as u32)?;
        options.SetDestinationHeight((size.Height * scale).round().max(1.0) as u32)?;
        let output = InMemoryRandomAccessStream::new()?;
        page.RenderWithOptionsToStreamAsync(&output, &options)?
            .join()?;
        page.Close()?;
        let length = u32::try_from(output.Size()?)
            .map_err(|_| windows::core::Error::from(windows::Win32::Foundation::E_INVALIDARG))?;
        let reader = DataReader::CreateDataReader(&output.GetInputStreamAt(0)?)?;
        reader.LoadAsync(length)?.join()?;
        let mut png = vec![0; length as usize];
        reader.ReadBytes(&mut png)?;
        Ok((pages, png))
    };
    draw().map_err(|error| format!("Could not open this PDF: {error}"))
}

#[cfg(all(test, any(target_os = "windows", target_os = "macos")))]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    fn build_pdf(objects: &[String]) -> Vec<u8> {
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for offset in offsets {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        pdf
    }

    fn two_page_pdf() -> Vec<u8> {
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 5 0 R >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 5 0 R >>".into(),
            "<< /Length 0 >>\nstream\n\nendstream".into(),
        ])
    }

    fn two_page_text_pdf() -> Vec<u8> {
        let s1 = "BT /F1 12 Tf 20 260 Td (First page introduction text.) Tj ET";
        let s2 = "BT /F1 12 Tf 20 220 Td (Before. The reported result preserves the complete data. After.) Tj ET";
        build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << /Font << /F1 7 0 R >> >> /Contents 5 0 R >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{s1}\nendstream", s1.len()),
            format!("<< /Length {} >>\nstream\n{s2}\nendstream", s2.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        ])
    }

    #[test]
    fn renders_pdf_pages_and_reports_invalid_documents() {
        std::thread::spawn(|| {
            let pdf = two_page_pdf();
            for page in 0..2 {
                let (count, png) = render(&pdf, page).unwrap();
                assert_eq!(count, 2);
                assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
            }
            assert!(render(&pdf, 2).is_err());
            assert!(render(b"%PDF-1.4\ninvalid", 0).is_err());
        })
        .join()
        .unwrap();
    }

    #[test]
    fn a_small_page_fills_the_bitmap_it_is_rendered_into() {
        // A page far smaller than the 1400 px bitmap, painted black edge to
        // edge: it must reach the corners instead of sitting small in the middle.
        let content = "0 g 0 0 200 300 re f";
        let pdf = build_pdf(&[
            "<< /Type /Catalog /Pages 2 0 R >>".into(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 300] /Resources << >> /Contents 4 0 R >>".into(),
            format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()),
        ]);
        std::thread::spawn(move || {
            let (_, png) = render(&pdf, 0).unwrap();
            let image = image::load_from_memory(&png).unwrap().to_rgba8();
            let (w, h) = image.dimensions();
            for (x, y) in [(2, 2), (w - 3, 2), (2, h - 3), (w - 3, h - 3), (w / 2, h / 2)] {
                let pixel = image.get_pixel(x, y).0;
                assert!(pixel[0] < 40, "({x}, {y}) is {pixel:?}, not page ink");
            }
        })
        .join()
        .unwrap();
    }

    #[test]
    fn extracts_positioned_spans_and_merges_line_highlights() {
        let pdf = two_page_text_pdf();
        let spans = page_spans(&pdf, 1);
        assert!(!spans.is_empty());
        let words: Vec<&str> = spans.iter().map(|s| s.text.as_str()).collect();
        let located =
            locate_in_items(&words, "The reported result preserves the complete data.").unwrap();
        let rects = merge_span_rects(&spans[located.start_item..=located.end_item]);
        assert!(!rects.is_empty());
        let [x, y, w, h] = rects[0];
        assert!(x >= 0.0 && x < 1.0);
        assert!(y >= 0.0 && y < 1.0);
        assert!(w > 0.02 && h > 0.01);
    }

    #[gpui::test]
    fn citation_focus_waits_for_page_count_and_selects_quoted_text(cx: &mut TestAppContext) {
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| PdfView::new("paper.pdf".into(), two_page_pdf(), cx))
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let quote = "The reported result preserves the complete data.";
                view.text = Some(monocode_core::papers::PaperText {
                    pages: vec!["The first page.".into(), format!("Before. {quote} After.")],
                    ..Default::default()
                });
                view.focus_passage(2, Some(quote.into()), cx);
                assert_eq!(view.page, 0);
                assert!(view.pending_focus.is_some());
                view.pages = 2;
                view.apply_focus(cx);
                assert_eq!(view.page, 1);
                assert!(view.text_mode);
                assert!(view.pending_focus.is_none());
                let input = view.text_input.read(cx);
                assert_eq!(&input.content[input.selection_range()], quote);
                assert!(view.selected_quote(cx).unwrap().contains("(page 2)"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn on_page_citation_and_selection_highlight_in_page_mode_and_send_to_chat(
        cx: &mut TestAppContext,
    ) {
        let pdf = two_page_text_pdf();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| PdfView::new("study.pdf".into(), pdf, cx))
            })
            .unwrap()
        });
        window
            .update(cx, |view, _, cx| {
                let quote = "The reported result preserves the complete data.";
                view.pages = 2;
                view.text = Some(extract_text_with_fallback(&view.bytes).unwrap());
                view.focus_passage(2, Some(quote.into()), cx);
                assert_eq!(view.page, 1);
                assert!(
                    !view.text_mode,
                    "stays in visual page view when x/y highlight is found"
                );
                assert!(!view.focus_rects.is_empty());
                let cited_quote = view.selected_quote(cx).unwrap();
                assert!(cited_quote.contains(quote));
                assert!(cited_quote.contains("- study.pdf (page 2)"));

                // Interactive span selection on the visual page also produces a chat quote
                view.select_span_range(1, 3, cx);
                assert!(!view.selection_rects.is_empty());
                let manual_quote = view.selected_quote(cx).unwrap();
                assert!(manual_quote.contains("The reported result"));
                assert!(manual_quote.contains("- study.pdf (page 2)"));

                // Search finds matches across pages and highlights them on the page
                view.search_input.update(cx, |input, cx| {
                    input.set_text("reported result".to_string(), cx)
                });
                view.update_search(true, cx);
                assert_eq!(view.search_matches.len(), 1);
                assert_eq!(view.page, 1);
                assert!(!view.search_rects.is_empty());
            })
            .unwrap();
    }
}
