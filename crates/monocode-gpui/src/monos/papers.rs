//! A cited paper opens in the Mono's details panel, beside its documents,
//! instead of in a code pane. Reading the PDF stays off the UI thread.
use super::*;
use crate::file_editor::pdf::{PdfEvent, PdfView};

pub(super) struct PaperPage {
    pub(super) title: String,
    pub(super) view: Entity<PdfView>,
    _events: Subscription,
}

impl Monos {
    pub(crate) fn open_paper(
        &mut self,
        path: String,
        page: usize,
        passage: Option<String>,
        title: String,
        cx: &mut Context<Self>,
    ) {
        self.route = Some(Route::Paper(path.clone()));
        self.overview_tab = OverviewTab::Documents;
        self.show_overview(cx);
        cx.notify();
        if let Some(paper) = self.papers.get(&path) {
            paper
                .view
                .update(cx, |view, cx| view.focus_passage(page, passage, cx));
            return;
        }
        if !self.loading_papers.insert(path.clone()) {
            return;
        }
        let file = PathBuf::from(&path);
        let work = cx
            .background_executor()
            .spawn(async move { std::fs::read(&file).map_err(|e| e.to_string()) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.loading_papers.remove(&path);
                let Ok(bytes) = result else {
                    cx.notify();
                    return;
                };
                let view = cx.new(|cx| PdfView::new(path.clone(), bytes, cx));
                let events = cx.subscribe(&view, |this, _, event: &PdfEvent, cx| {
                    this.quote_in_chat(event.0.clone(), cx)
                });
                view.update(cx, |view, cx| view.focus_passage(page, passage, cx));
                this.papers.insert(
                    path,
                    PaperPage {
                        title,
                        view,
                        _events: events,
                    },
                );
                cx.notify();
            });
        })
        .detach();
    }

    /// A citation the pointer rests on moves the paper shown beside the chat
    /// to its passage. False when that paper is not on screen.
    pub(crate) fn point_open_paper(
        &mut self,
        path: &str,
        page: usize,
        passage: Option<String>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.route.as_ref() != Some(&Route::Paper(path.to_owned())) {
            return false;
        }
        let Some(paper) = self.papers.get(path) else {
            return false;
        };
        paper
            .view
            .update(cx, |view, cx| view.focus_passage(page, passage, cx));
        true
    }

    /// "Add to chat" from a paper waits in the composer as a quote chip
    /// that names the file and page; the user sends it with their question.
    fn quote_in_chat(&mut self, quote: String, cx: &mut Context<Self>) {
        let Some(chat) = self
            .active
            .as_ref()
            .and_then(|id| self.chats.get(id))
            .cloned()
        else {
            return;
        };
        let Some(window) = self.parent_window else {
            return;
        };
        cx.defer(move |cx| {
            let _ = window.update(cx, |_, window, cx| {
                chat.update(cx, |chat, cx| chat.mono_quote(&quote, window, cx))
            });
        });
    }

    pub(super) fn paper_page(&self, path: &str, cx: &mut Context<Self>) -> AnyElement {
        let loading = self.loading_papers.contains(path);
        let paper = self.papers.get(path);
        let title = paper.map_or_else(|| "Paper".to_owned(), |paper| paper.title.clone());
        let body = match paper {
            Some(paper) => paper.view.clone().into_any_element(),
            None => panels::empty(if loading {
                "Opening paper…"
            } else {
                "Paper unavailable"
            }),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.page_header(&title, cx))
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }
}

/// What the panel shows of a cited paper without a PDF (PaperReader.tsx):
/// its authors and abstract, with the cited passage marked.
pub(crate) struct PaperRecord {
    pub(super) meta: Option<monocode_core::papers::PaperMeta>,
    pub(super) passage: Option<String>,
}

impl Monos {
    pub(crate) fn open_paper_record(
        &mut self,
        url: String,
        passage: Option<String>,
        cx: &mut Context<Self>,
    ) {
        self.route = Some(Route::PaperRecord(url.clone()));
        self.overview_tab = OverviewTab::Documents;
        self.show_overview(cx);
        if let Some(record) = self.paper_records.get_mut(&url) {
            record.passage = passage;
            cx.notify();
            return;
        }
        self.paper_records.insert(
            url.clone(),
            PaperRecord {
                meta: None,
                passage,
            },
        );
        cx.notify();
        let lookup = url.clone();
        let work = cx.background_executor().spawn(async move {
            let store =
                monocode_core::papers::PaperStore::new(monocode_core::papers::cache_dir().ok()?);
            monocode_core::papers::cached_record(&store, &lookup)
        });
        cx.spawn(async move |this, cx| {
            let meta = work.await;
            let _ = this.update(cx, |this, cx| {
                if let Some(record) = this.paper_records.get_mut(&url) {
                    record.meta = meta;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(super) fn paper_record_page(&self, url: &str, cx: &mut Context<Self>) -> AnyElement {
        let record = self.paper_records.get(url);
        let meta = record.and_then(|record| record.meta.as_ref());
        let title = meta.map_or_else(|| "Paper".to_owned(), |meta| meta.paper.title.clone());
        let Some(meta) = meta else {
            return div()
                .size_full()
                .flex()
                .flex_col()
                .child(self.page_header(&title, cx))
                .child(panels::empty("Paper unavailable"))
                .into_any_element();
        };
        let paper = &meta.paper;
        let details = [
            (!paper.authors.is_empty()).then(|| paper.authors.join(", ")),
            paper.year.map(|year| year.to_string()),
            paper.venue.clone(),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let abstract_text = paper.r#abstract.clone().unwrap_or_default();
        let marked = record
            .and_then(|record| record.passage.as_deref())
            .and_then(|passage| monocode_core::papers::locate_in_items(&[&abstract_text], passage))
            .map(|found| found.start_offset..found.end_offset);
        let body = gpui::StyledText::new(abstract_text.clone()).with_highlights(marked.map(
            |range| {
                (
                    range,
                    gpui::HighlightStyle {
                        background_color: Some(accent_color().opacity(0.16)),
                        underline: Some(gpui::UnderlineStyle {
                            thickness: px(1.5),
                            color: Some(accent_color()),
                            wavy: false,
                        }),
                        ..Default::default()
                    },
                )
            },
        ));
        let open = url.to_owned();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(self.page_header(&title, cx))
            .child(
                div()
                    .id("paper-record")
                    .debug_selector(|| "paper-record".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(21.))
                            .text_color(content(1.))
                            .child(title.clone()),
                    )
                    .when(!details.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.6))
                                .child(details),
                        )
                    })
                    .child(
                        div()
                            .rounded_md()
                            .bg(content(0.05))
                            .px_2p5()
                            .py_1p5()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .child(meta.reason.clone().unwrap_or_else(|| {
                                "The app has only this paper's abstract, not its full text.".into()
                            })),
                    )
                    .child(if abstract_text.is_empty() {
                        div()
                            .text_size(px(13.))
                            .text_color(content(0.5))
                            .child("No abstract is available.")
                            .into_any_element()
                    } else {
                        div()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(content(0.85))
                            .child(body)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .id("paper-record-open")
                            .role(Role::Link)
                            .aria_label("Open in browser")
                            .self_start()
                            .rounded_md()
                            .px_3()
                            .py_1p5()
                            .text_size(px(12.))
                            .hover(|s| s.bg(content(0.08)))
                            .child("Open in browser")
                            .on_click(move |_, _, cx| cx.open_url(&open)),
                    ),
            )
            .into_any_element()
    }
}
