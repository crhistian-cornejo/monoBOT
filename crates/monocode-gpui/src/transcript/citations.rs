//! "N sources" under a reply (CitationSources.tsx): the pages and papers it
//! cites, numbered as its chips are, folded until opened. The parsing is
//! core's (citations::collect).
use super::*;
use monocode_core::citations::{self, Source};

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
        // A paper's viewer underlines every passage its replies cited.
        if self.cited_noted.get(&turn) != Some(&sources.len()) {
            self.cited_noted.insert(turn, sources.len());
            let cited: Vec<_> = sources
                .iter()
                .filter_map(|s| Some((s.url.clone(), s.snippet.clone()?, s.number)))
                .collect();
            if !cited.is_empty() {
                cx.background_executor()
                    .spawn(async move { crate::citation_card::record(cited) })
                    .detach();
            }
        }
        let open = self.sources_open.contains(&turn);
        let stack: Vec<_> = sources
            .iter()
            .take(citations::STACK_MONOGRAMS)
            .enumerate()
            .map(|(ix, source)| {
                div()
                    .when(ix > 0, |d| d.ml(px(-4.)))
                    .child(crate::site_icons::badge(&source.host, &source.url, 20., cx))
            })
            .collect();
        let rows: Vec<_> = if open {
            sources
                .iter()
                .map(|source| crate::site_icons::badge(&source.host, &source.url, 16., cx))
                .collect()
        } else {
            Vec::new()
        };
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
            .child(div().flex().children(stack))
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
                        sources.into_iter().zip(rows).map(|(source, site)| {
                            let url = source.url.clone();
                            let passage = source.snippet.clone();
                            let point = Some((url.clone(), passage.clone()));
                            div()
                                .id(SharedString::from(format!(
                                    "source-{turn}-{}",
                                    source.number
                                )))
                                .role(Role::Link)
                                .tooltip(crate::citation_card::tip(
                                    url.clone(),
                                    Some(source.title.clone()),
                                    Some(source.number),
                                    passage.clone(),
                                ))
                                .on_hover(move |hovered, window, cx| {
                                    window.dispatch_action(
                                        Box::new(crate::citation_card::PointAtCitation(
                                            hovered.then(|| point.clone()).flatten(),
                                        )),
                                        cx,
                                    );
                                })
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
                                .child(site)
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
