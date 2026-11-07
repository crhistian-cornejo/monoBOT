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
                    if let Some(id) = this.active.clone() {
                        this.enqueue(&id, event.0.clone(), Vec::new(), false, cx);
                    }
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
