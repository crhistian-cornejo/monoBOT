//! A cited source's hover card (citationContext.tsx SourceCard): the site,
//! the title, the quoted passage and, for a paper whose PDF the app keeps,
//! a picture of where the passage sits on its page with the words marked
//! (PaperPassagePreview.tsx). Resting on a citation also points a viewer
//! that already shows the paper at the passage.
use crate::theme::{accent_color, content, rail_color};
use gpui::{
    AnyView, App, AppContext as _, Context, IntoElement, ParentElement, Render, Styled, Task,
    Window, div, img, prelude::*, px, relative,
};
use monocode_core::papers::{self, PaperStore};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock},
};

const CARD_WIDTH: f32 = 300.;
const PREVIEW_WIDTH: f32 = CARD_WIDTH - 24.;
/// Drawn twice as wide as shown, for a sharp picture on Retina screens.
const PREVIEW_PIXELS: f32 = PREVIEW_WIDTH * 2.;
const CROP_LIMIT: usize = 40;

/// Where a cited passage is in its paper, drawn: the cropped part of the
/// page and the passage's boxes over it, as shares of the crop.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Preview {
    pub image: PathBuf,
    pub page: usize,
    /// Height over width.
    pub aspect: f32,
    pub marks: Vec<[f32; 4]>,
}

type Crops = Mutex<HashMap<(String, String), Option<Arc<Preview>>>>;
fn crops() -> &'static Crops {
    static CROPS: OnceLock<Crops> = OnceLock::new();
    CROPS.get_or_init(Default::default)
}

/// Draws the part of the page a cited passage is on. None for a source that
/// is not a paper the app has the PDF of, or a passage it cannot find.
pub(crate) fn draw_preview(url: &str, passage: &str) -> Option<Preview> {
    let store = PaperStore::new(papers::cache_dir().ok()?);
    let (path, page, _) = papers::cached_citation(&store, url, Some(passage))?;
    let bytes = std::fs::read(&path).ok()?;
    let spans = crate::file_editor::pdf::page_spans(&bytes, page - 1);
    let words: Vec<&str> = spans.iter().map(|span| span.text.as_str()).collect();
    let found = papers::locate_in_items(&words, passage)?;
    let end = found.end_item.min(spans.len().checked_sub(1)?);
    let marks = papers::merge_span_rects(&spans[found.start_item..=end]);
    // A first draw at a known width gives the page's size in pixels.
    let (_, png) = crate::file_editor::pdf::render_width(&bytes, page - 1, 1224.).ok()?;
    let full = image::load_from_memory(&png).ok()?.to_rgba8();
    let (w, h) = (full.width() as f32, full.height() as f32);
    let scale = w / 612.;
    let marks_px: Vec<[f32; 4]> = marks
        .iter()
        .map(|[x, y, mw, mh]| [x * w, y * h, mw * w, mh * h])
        .collect();
    let [cx, cy, cw, ch] =
        papers::crop_around(&marks_px, (w, h), 18. * scale, 220. * scale, 220. * scale)?;
    let crop = image::imageops::crop_imm(
        &full,
        cx.max(0.) as u32,
        cy.max(0.) as u32,
        (cw as u32).max(1),
        (ch as u32).max(1),
    )
    .to_image();
    let crop = if crop.width() as f32 > PREVIEW_PIXELS * 1.2 {
        let height = (crop.height() as f32 * PREVIEW_PIXELS / crop.width() as f32).max(1.);
        image::imageops::resize(
            &crop,
            PREVIEW_PIXELS as u32,
            height as u32,
            image::imageops::FilterType::Triangle,
        )
    } else {
        crop
    };
    let dir = std::env::temp_dir().join("monobot-citation-previews");
    std::fs::create_dir_all(&dir).ok()?;
    let name = {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        (url, passage, path.to_string_lossy().as_ref()).hash(&mut hasher);
        format!("{:016x}.png", hasher.finish())
    };
    let image_path = dir.join(name);
    crop.save(&image_path).ok()?;
    Some(Preview {
        image: image_path,
        page,
        aspect: ch / cw,
        marks: marks_px
            .iter()
            .map(|[x, y, mw, mh]| [(x - cx) / cw, (y - cy) / ch, mw / cw, mh / ch])
            .collect(),
    })
}

pub(crate) struct CitationCard {
    url: String,
    host: String,
    title: String,
    number: Option<u32>,
    passage: Option<String>,
    preview: Option<Arc<Preview>>,
    _drawing: Option<Task<()>>,
}

impl CitationCard {
    pub(crate) fn new(
        url: String,
        title: Option<String>,
        number: Option<u32>,
        passage: Option<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let host = monocode_core::citations::host(&url);
        let title = title
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| host.clone());
        let mut card = Self {
            url,
            host,
            title,
            number,
            passage,
            preview: None,
            _drawing: None,
        };
        card.draw(cx);
        card
    }

    fn draw(&mut self, cx: &mut Context<Self>) {
        let Some(passage) = self.passage.clone() else {
            return;
        };
        let key = (self.url.clone(), passage.clone());
        if let Some(known) = crops()
            .lock()
            .ok()
            .and_then(|crops| crops.get(&key).cloned())
        {
            self.preview = known;
            return;
        }
        let url = self.url.clone();
        let work = cx
            .background_executor()
            .spawn(async move { draw_preview(&url, &passage).map(Arc::new) });
        self._drawing = Some(cx.spawn(async move |this, cx| {
            let preview = work.await;
            if let Ok(mut crops) = crops().lock() {
                if crops.len() >= CROP_LIMIT {
                    crops.clear();
                }
                crops.insert(key, preview.clone());
            }
            let _ = this.update(cx, |this, cx| {
                this.preview = preview;
                cx.notify();
            });
        }));
    }
}

impl Render for CitationCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview = self.preview.clone();
        div()
            .debug_selector(|| "citation-card".into())
            .w(px(CARD_WIDTH))
            .p_3()
            .rounded_lg()
            .bg(rail_color())
            .border_1()
            .border_color(content(0.15))
            .shadow_lg()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .text_size(px(11.))
                    .text_color(content(0.55))
                    .child(crate::site_icons::badge(&self.host, &self.url, 14., cx))
                    .child(div().flex_1().min_w_0().truncate().child(self.host.clone()))
                    .children(
                        self.number
                            .map(|n| div().flex_shrink_0().child(n.to_string())),
                    ),
            )
            .child(
                div()
                    .mt_1p5()
                    .line_clamp(2)
                    .text_size(px(13.))
                    .line_height(px(18.))
                    .text_color(content(1.))
                    .child(self.title.clone()),
            )
            .children(self.passage.clone().map(|passage| {
                div()
                    .mt_1()
                    .line_clamp(3)
                    .text_size(px(12.))
                    .line_height(px(17.))
                    .text_color(content(0.6))
                    .child(passage)
            }))
            .children(preview.map(|preview| {
                let height = PREVIEW_WIDTH * preview.aspect;
                div()
                    .mt_2()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .debug_selector(|| "citation-preview".into())
                            .relative()
                            .w(px(PREVIEW_WIDTH))
                            .h(px(height))
                            .rounded_md()
                            .overflow_hidden()
                            .bg(gpui::white())
                            .border_1()
                            .border_color(content(0.08))
                            .child(img(preview.image.clone()).size_full())
                            .children(preview.marks.iter().map(|[x, y, w, h]| {
                                div()
                                    .absolute()
                                    .left(relative(*x))
                                    .top(relative(*y))
                                    .w(relative(*w))
                                    .h(relative(*h))
                                    .rounded(px(1.))
                                    .bg(accent_color().opacity(0.3))
                            })),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(11.))
                            .text_color(content(0.5))
                            .child(format!(
                                "Page {} · click to open the paper here",
                                preview.page
                            )),
                    )
            }))
    }
}

/// The `.tooltip` builder for a citation: its card.
pub(crate) fn tip(
    url: String,
    title: Option<String>,
    number: Option<u32>,
    passage: Option<String>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    move |_, cx| {
        let (url, title, passage) = (url.clone(), title.clone(), passage.clone());
        cx.new(|cx| CitationCard::new(url, title, number, passage, cx))
            .into()
    }
}

/// Resting on a citation of a paper open beside the chat points that paper
/// at the passage; leaving it (None) cancels a point still waiting.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = citation_card, no_json)]
pub(crate) struct PointAtCitation(pub Option<(String, Option<String>)>);

/// Paths whose PDF a citation's link names, looked up once.
type Resolved = Mutex<HashMap<String, Option<(String, usize)>>>;
fn resolved() -> &'static Resolved {
    static RESOLVED: OnceLock<Resolved> = OnceLock::new();
    RESOLVED.get_or_init(Default::default)
}

/// The cached PDF and page a citation names, off the UI thread.
pub(crate) fn resolve(url: &str, passage: Option<&str>) -> Option<(String, usize)> {
    let key = format!("{url}\u{0}{}", passage.unwrap_or(""));
    if let Some(known) = resolved()
        .lock()
        .ok()
        .and_then(|map| map.get(&key).cloned())
    {
        return known;
    }
    let store = PaperStore::new(papers::cache_dir().ok()?);
    let found = papers::cached_citation(&store, url, passage)
        .map(|(path, page, _)| (path.to_string_lossy().into_owned(), page));
    if let Ok(mut map) = resolved().lock() {
        if map.len() > 400 {
            map.clear();
        }
        map.insert(key, found.clone());
    }
    found
}

/// Notes the passages a reply cites from papers the app keeps, so each
/// paper's viewer underlines them all (recordPaperPassage).
pub(crate) fn record(sources: Vec<(String, String, u32)>) {
    let Ok(dir) = papers::cache_dir() else {
        return;
    };
    let store = PaperStore::new(dir);
    for (url, passage, number) in sources {
        if let Some((path, _)) = resolve(&url, None) {
            let _ = store.record_cited(std::path::Path::new(&path), &passage, Some(number));
        }
    }
}
