//! The legwork behind a Mono's answer, above its step list in the Activity
//! tab: the searches it ran, the pages it opened and which of them it cited
//! (MonoActivityView.tsx's ResearchSummary).
use super::*;
use monocode_core::research::{Source, TurnResearch};

/// Sources shown before "N more".
const FIRST_SOURCES: usize = 6;

fn plural(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

impl Monos {
    pub(super) fn research_section(
        &mut self,
        research: &TurnResearch,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if research.is_empty() {
            return None;
        }
        let all = self.activity_sources_all;
        let sources = &research.sources;
        let cited = sources.iter().filter(|s| s.cited.is_some()).count();
        let read = sources.iter().filter(|s| s.read).count();
        let stats = [
            (!research.searches.is_empty())
                .then(|| plural(research.searches.len(), "search", "searches")),
            (read > 0).then(|| format!("{} read", plural(read, "page", "pages"))),
            (cited > 0).then(|| format!("{cited} cited")),
            (research.skipped > 0).then(|| format!("{} passed over", research.skipped)),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
        let shown = if all {
            sources.len()
        } else {
            FIRST_SOURCES.min(sources.len())
        };
        let hidden = sources.len() - shown;
        let rows = sources
            .iter()
            .take(shown)
            .enumerate()
            .map(|(ix, source)| self.source_row(ix, source, cx))
            .collect::<Vec<_>>();
        Some(
            div()
                .id("mono-research")
                .debug_selector(|| "mono-research".into())
                .role(Role::Group)
                .aria_label("Research")
                .mx_1()
                .mb_3()
                .px_2()
                .py_2()
                .border_1()
                .border_color(content(0.08))
                .bg(content(0.02))
                .rounded_md()
                .flex()
                .flex_col()
                .child(
                    div()
                        .px_1()
                        .pb(px(6.))
                        .text_size(px(11.))
                        .text_color(content(0.45))
                        .child(stats),
                )
                .children(research.searches.iter().map(|search| {
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_w_0()
                        .px_1()
                        .py_1()
                        .text_size(px(12.))
                        .text_color(content(0.75))
                        .child(crate::theme::icon("search", 14.).text_color(content(0.4)))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(search.query.clone()),
                        )
                        .when(search.results > 0, |d| {
                            d.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.))
                                    .text_color(content(0.4))
                                    .child(search.results.to_string()),
                            )
                        })
                }))
                .when(!rows.is_empty(), |d| {
                    d.child(
                        div()
                            .flex()
                            .flex_col()
                            .when(!research.searches.is_empty(), |d| {
                                d.mt_1().pt_1().border_t_1().border_color(content(0.08))
                            })
                            .children(rows),
                    )
                })
                .when(hidden > 0 || all, |d| {
                    d.child(
                        div()
                            .id("mono-research-more")
                            .role(Role::Button)
                            .aria_label(if all {
                                "Show fewer sources"
                            } else {
                                "Show all sources"
                            })
                            .mt(px(2.))
                            .px_1()
                            .py_1()
                            .rounded_md()
                            .text_size(px(11.))
                            .text_color(content(0.5))
                            .hover(|s| s.bg(content(0.08)).text_color(content(0.8)))
                            .child(if all {
                                "Fewer".to_owned()
                            } else {
                                format!("{hidden} more")
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.activity_sources_all = !this.activity_sources_all;
                                cx.notify();
                            })),
                    )
                })
                .into_any_element(),
        )
    }

    fn source_row(&self, ix: usize, source: &Source, cx: &mut Context<Self>) -> AnyElement {
        let url = source.url.clone();
        div()
            .id(("mono-research-source", ix))
            .role(Role::Link)
            .aria_label(source.title.clone().unwrap_or_else(|| source.host.clone()))
            .flex()
            .items_center()
            .gap_2()
            .min_w_0()
            .px_1()
            .py(px(6.))
            .rounded_md()
            .text_size(px(12.))
            .hover(|s| s.bg(content(0.08)))
            .child(crate::site_icons::badge(&source.host, &source.url, 16., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .truncate()
                            .text_color(content(if source.cited.is_some() { 1. } else { 0.7 }))
                            .child(source.title.clone().unwrap_or_else(|| source.host.clone())),
                    )
                    .when(source.title.is_some(), |d| {
                        d.child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(content(0.4))
                                .child(source.host.clone()),
                        )
                    }),
            )
            .child(match source.cited {
                Some(number) => div()
                    .flex_shrink_0()
                    .px(px(6.))
                    .rounded_full()
                    .bg(accent_color().opacity(0.15))
                    .text_size(px(10.))
                    .text_color(accent_color())
                    .child(number.to_string()),
                None => div()
                    .flex_shrink_0()
                    .text_size(px(10.))
                    .text_color(content(0.35))
                    .child(if source.read { "read" } else { "result" }),
            })
            .child(crate::theme::icon("external-link", 12.).text_color(content(0.25)))
            .on_click(move |_, window, cx| {
                crate::markdown::open_citation(&url, None, window, cx);
            })
            .into_any_element()
    }
}
