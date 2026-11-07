//! "N sources" under a reply (CitationSources.tsx): the pages and papers it
//! cites, numbered as its chips are, folded until opened. The parsing is
//! core's (citations::collect).
use super::*;
use monocode_core::citations::{self, Source};

/// The letter a site shows in place of its icon (CitationMonogram).
fn monogram(host: &str, size: f32) -> gpui::Div {
    div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .bg(content(0.12))
        .border_1()
        .border_color(background_base())
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(size * 0.5))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(content(0.7))
        .child(citations::monogram(host))
}

impl TranscriptView {
    /// The toggle and, once opened, the list; none when the reply cites nothing.
    pub(super) fn citation_sources(
        &mut self,
        turn: usize,
        output: &str,
        cx: &mut Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let sources: Vec<Source> = citations::collect(output);
        if sources.is_empty() {
            return None;
        }
        let open = self.sources_open.contains(&turn);
        let toggle = div()
            .id(("citation-sources", turn))
            .debug_selector(|| "citation-sources".into())
            .role(Role::Button)
            .aria_label(citations::sources_label(sources.len()))
            .aria_expanded(open)
            .flex()
            .items_center()
            .gap_2()
            .rounded_full()
            .py_1()
            .pl(px(4.))
            .pr(px(10.))
            .text_size(px(12.))
            .text_color(content(0.7))
            .hover(|s| s.bg(content(0.08)))
            .child(
                div().flex().children(
                    sources
                        .iter()
                        .take(citations::STACK_MONOGRAMS)
                        .enumerate()
                        .map(|(ix, source)| {
                            monogram(&source.host, 20.).when(ix > 0, |d| d.ml(px(-4.)))
                        }),
                ),
            )
            .child(citations::sources_label(sources.len()))
            .child(
                icon("chevron-down", 14.)
                    .text_color(content(0.4))
                    .when(open, |i| {
                        i.with_transformation(gpui::Transformation::rotate(gpui::percentage(0.5)))
                    }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.sources_open.remove(&turn) {
                    this.sources_open.insert(turn);
                }
                cx.notify();
            }));
        Some(
            div()
                .mt_3()
                .flex()
                .flex_col()
                .items_start()
                .child(toggle)
                .when(open, |d| {
                    d.child(div().w_full().pt_2().flex().flex_col().gap_1().children(
                        sources.into_iter().map(|source| {
                            let url = source.url.clone();
                            let passage = source.snippet.clone();
                            div()
                                .id(SharedString::from(format!(
                                    "source-{turn}-{}",
                                    source.number
                                )))
                                .role(Role::Link)
                                .aria_label(source.title.clone())
                                .flex()
                                .items_center()
                                .gap(px(10.))
                                .rounded_md()
                                .px_2()
                                .py(px(6.))
                                .text_size(px(13.))
                                .text_color(content(1.))
                                .hover(|s| s.bg(content(0.08)))
                                .child(
                                    div()
                                        .w(px(12.))
                                        .flex_shrink_0()
                                        .text_right()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(source.number.to_string()),
                                )
                                .child(monogram(&source.host, 16.))
                                .child(div().flex_1().min_w_0().truncate().child(source.title))
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(11.))
                                        .text_color(content(0.4))
                                        .child(source.host),
                                )
                                .child(icon("external-link", 14.).text_color(content(0.25)))
                                .on_click(move |_, window, cx| {
                                    crate::markdown::open_citation(
                                        &url,
                                        passage.as_deref(),
                                        window,
                                        cx,
                                    );
                                })
                        }),
                    ))
                })
                .into_any_element(),
        )
    }
}
