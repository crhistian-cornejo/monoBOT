//! Installed-update card and bundled What's new content. The app updater itself
//! is a separate backend concern; a successful installer records the marker.
use crate::{
    markdown::{self, MarkdownRow, Tone},
    markdown_model,
    theme::*,
};
use gpui::{Context, EventEmitter, FontWeight, Render, Role, Window, div, img, prelude::*, px};
use monocode_core::store::Store;
use std::time::Instant;
const MARKER: &str = "monocode.installedUpdate";

/// Show the marker once, after the expected version is actually running. The
/// settings abstraction has no delete operation, so an empty value is cleared.
pub(crate) fn consume_installed_update(store: &Store) -> Result<Option<String>, String> {
    let Some(raw) = store.setting(MARKER)? else {
        return Ok(None);
    };
    if raw.is_empty() {
        return Ok(None);
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&raw).ok();
    let version = parsed
        .as_ref()
        .and_then(|v| v["version"].as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned);
    // A cancelled/failed installer must never claim that the app was updated.
    if version.as_deref() != Some(super::updater::version()) {
        return Ok(None);
    }
    store.save_setting(MARKER, "")?;
    Ok(version)
}
pub(crate) enum AppUpdateEvent {
    Open(String),
    Dismiss,
}
pub(crate) struct InstalledUpdateNotice {
    version: Option<String>,
}
impl EventEmitter<AppUpdateEvent> for InstalledUpdateNotice {}
impl InstalledUpdateNotice {
    pub(crate) fn new(version: Option<String>) -> Self {
        Self { version }
    }
    pub(crate) fn set_version(&mut self, version: Option<String>, cx: &mut Context<Self>) {
        if self.version != version {
            self.version = version;
            cx.notify();
        }
    }
    pub(crate) fn is_visible(&self) -> bool {
        self.version.is_some()
    }
}
impl Render for InstalledUpdateNotice {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(version) = self.version.clone() else {
            return div().into_any_element();
        };
        let label = format!("Updated to {version}");
        div()
            .id("installed-update-notice")
            .role(Role::Status)
            .relative()
            .overflow_hidden()
            .rounded(px(8.))
            .bg(content(0.12))
            .child(
                div()
                    .id("open-installed-update")
                    .role(Role::Button)
                    .aria_label(label.clone())
                    .cursor_pointer()
                    .flex()
                    .w_full()
                    .items_start()
                    .gap(px(8.))
                    .rounded(px(8.))
                    .p(px(8.))
                    .pr(px(32.))
                    .hover(|s| {
                        let mut accent = accent_color();
                        accent.a = 0.1;
                        s.bg(accent)
                    })
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(AppUpdateEvent::Open(version.clone()))
                    }))
                    .child(
                        div()
                            .mt(px(2.))
                            .size(px(18.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(img("monocode.png").size(px(16.))),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child(label),
                            )
                            .child(
                                div()
                                    .mt(px(2.))
                                    .text_size(px(11.))
                                    .text_color(content(0.5))
                                    .child("What's new"),
                            ),
                    ),
            )
            .child(
                div()
                    .id("dismiss-installed-update")
                    .role(Role::Button)
                    .aria_label("Dismiss update notification")
                    .tooltip(chrome_tip("Dismiss update notification", None))
                    .debug_selector(|| "dismiss-installed-update".into())
                    // The X sits over the clickable card; keep its click from opening What's new.
                    .block_mouse_except_scroll()
                    .absolute()
                    .right(px(4.))
                    .top(px(4.))
                    .size(px(24.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(content(0.45))
                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(AppUpdateEvent::Dismiss)))
                    .child(icon("x", 14.)),
            )
            .into_any_element()
    }
}
struct ReleaseNotes {
    date: Option<String>,
    markdown: String,
}
fn release_notes(version: &str) -> Option<ReleaseNotes> {
    let version = version.trim();
    if version.is_empty() || version == "Unreleased" {
        return None;
    }
    let source = include_str!("../../../../CHANGELOG.md");
    let heading = format!("## [{version}]");
    let mut found = false;
    let mut date = None;
    let mut body = Vec::new();
    for line in source.lines() {
        if !found {
            let Some(suffix) = line.strip_prefix(&heading) else {
                continue;
            };
            if suffix.is_empty() {
                found = true;
            } else if let Some(iso) = suffix.strip_prefix(" - ")
                && iso.len() == 10
                && iso.bytes().enumerate().all(|(i, b)| {
                    if i == 4 || i == 7 {
                        b == b'-'
                    } else {
                        b.is_ascii_digit()
                    }
                })
            {
                date = Some(format_date(iso));
                found = true;
            }
        } else if line.starts_with("## ") {
            break;
        } else {
            body.push(line);
        }
    }
    found.then(|| ReleaseNotes {
        date,
        markdown: body.join("\n").trim().to_owned(),
    })
}
fn format_date(iso: &str) -> String {
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let month = iso[5..7]
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_sub(1))
        .and_then(|n| months.get(n));
    match month {
        Some(month) => format!(
            "{} {month} {}",
            iso[8..10].parse::<u32>().unwrap_or(0),
            &iso[..4]
        ),
        None => iso.into(),
    }
}
pub(crate) enum WhatsNewEvent {
    Close,
}
pub(crate) struct WhatsNew {
    version: String,
    date: Option<String>,
    rows: Vec<MarkdownRow>,
    highlights: &'static [super::highlights::Highlight],
}
impl EventEmitter<WhatsNewEvent> for WhatsNew {}
impl WhatsNew {
    pub(crate) fn set_release_notes(&mut self, body: &str, cx: &mut Context<Self>) {
        if !body.trim().is_empty() {
            self.rows = markdown_model::parse_full(body)
                .blocks
                .iter()
                .flat_map(|b| markdown::rows(&b.block))
                .collect();
            cx.notify();
        }
    }
    pub(crate) fn new(version: String) -> Self {
        let notes = release_notes(&version);
        let date = notes.as_ref().and_then(|n| n.date.clone());
        let rows = notes
            .map(|notes| {
                markdown_model::parse_full(&notes.markdown)
                    .blocks
                    .iter()
                    .flat_map(|b| markdown::rows(&b.block))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            highlights: super::highlights::of(&version),
            version,
            date,
            rows,
        }
    }
}

/// A highlight's picture: its screenshot fading into the card over a
/// blurred, dimmed copy of itself, or, without one, its icon on a dithered
/// accent banner.
fn banner(highlight: &super::highlights::Highlight, ix: usize) -> gpui::AnyElement {
    let fade = {
        let mut clear = crate::theme::pane_fill();
        clear.a = 0.;
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(px(48.))
            .bg(gpui::linear_gradient(
                180.,
                gpui::linear_color_stop(clear, 0.),
                gpui::linear_color_stop(crate::theme::pane_fill(), 1.),
            ))
    };
    match highlight.image {
        Some(image) => div()
            .relative()
            .h(px(188.))
            .overflow_hidden()
            .child(
                // The copy behind, larger and faint, reads as a blur.
                img(image)
                    .absolute()
                    .top(px(-24.))
                    .left(px(-24.))
                    .right(px(-24.))
                    .bottom(px(-24.))
                    .object_fit(gpui::ObjectFit::Cover)
                    .opacity(0.28),
            )
            .child(
                img(image)
                    .absolute()
                    .top(px(18.))
                    .left(px(28.))
                    .right(px(28.))
                    .bottom(px(-40.))
                    .rounded(px(8.))
                    .object_fit(gpui::ObjectFit::Cover),
            )
            .child(fade)
            .into_any_element(),
        None => div()
            .relative()
            .h(px(112.))
            .overflow_hidden()
            .bg(accent_color().opacity(0.07))
            .child(highlight_dither(ix).absolute().inset_0())
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(52.))
                            .rounded(px(14.))
                            .bg(crate::theme::pane_fill())
                            .border_1()
                            .border_color(accent_color().opacity(0.35))
                            .shadow(vec![gpui::BoxShadow {
                                color: accent_color().opacity(0.35),
                                offset: gpui::point(px(0.), px(0.)),
                                blur_radius: px(24.),
                                spread_radius: px(0.),
                                inset: false,
                            }])
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon(highlight.icon, 24.).text_color(accent_color())),
                    ),
            )
            .child(fade)
            .into_any_element(),
    }
}

/// The banner's halftone: accent dots densest toward one side, the side
/// alternating from card to card.
fn highlight_dither(ix: usize) -> gpui::Canvas<()> {
    use crate::visuals::dither;
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let mut ink = accent_color();
            ink.a *= if dither::light() { 0.55 } else { 0.5 };
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let right = ix.is_multiple_of(2);
            dither::Grid::new(bounds.origin, w, h).paint(window, |col, row| {
                let (x, y) = dither::Grid::center(col, row);
                let across = if right { x / w } else { 1. - x / w };
                let tone = (across * 0.9 - y / h * 0.35).clamp(0., 1.).powf(1.6);
                dither::inked(col, row, tone).then_some(ink)
            });
        },
    )
}

fn highlight_card(highlight: &super::highlights::Highlight, ix: usize) -> gpui::AnyElement {
    use super::highlights::Tag;
    let tag_color = match highlight.tag {
        Tag::New => accent_color(),
        Tag::Improved => crate::theme::info(),
        Tag::Fixed => crate::theme::success(),
    };
    div()
        .id(("whats-new-highlight", ix))
        .debug_selector(move || format!("whats-new-highlight-{ix}"))
        .role(Role::Article)
        .aria_label(highlight.title)
        .rounded(px(12.))
        .border_1()
        .border_color(content(0.08))
        .overflow_hidden()
        .bg(content(0.02))
        .child(banner(highlight, ix))
        .child(
            div()
                .px(px(16.))
                .pt(px(4.))
                .pb(px(16.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(
                    div().flex().child(
                        div()
                            .h(px(20.))
                            .px(px(7.))
                            .rounded(px(6.))
                            .flex()
                            .items_center()
                            .text_size(px(11.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(tag_color)
                            .bg(tag_color.opacity(0.12))
                            .child(highlight.tag.label()),
                    ),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(highlight.title),
                )
                .child(
                    div()
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(content(0.7))
                        .child(highlight.body),
                ),
        )
        .into_any_element()
}

impl Render for WhatsNew {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let description = format!(
            "monoBOT {}{}",
            self.version,
            self.date
                .as_ref()
                .map(|d| format!(" · {d}"))
                .unwrap_or_default()
        );
        let mut body = div()
            .id("whats-new-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .py(px(16.));
        for (ix, highlight) in self.highlights.iter().enumerate() {
            body = body.child(
                div()
                    .when(ix > 0, |d| d.pt(px(14.)))
                    .child(highlight_card(highlight, ix)),
            );
        }
        if !self.highlights.is_empty() && !self.rows.is_empty() {
            body = body.child(
                div()
                    .pt(px(24.))
                    .pb(px(10.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(content(0.5))
                    .child("All changes"),
            );
        }
        if self.rows.is_empty() && self.highlights.is_empty() {
            body = body.child(
                div()
                    .text_size(px(13.))
                    .text_color(content(0.6))
                    .child("Release notes for this version are not available in this build."),
            );
        }
        for (ix, row) in self.rows.iter().enumerate() {
            body = body.child(div().pt(px(if ix == 0 { 0. } else { row.gap })).child(
                markdown::body(
                    row,
                    format!("whats-new-row-{ix}").into(),
                    Tone::Normal,
                    None,
                    Instant::now(),
                ),
            ));
        }
        div()
            .id("whats-new-dialog")
            .role(Role::Dialog)
            .aria_label("What's new")
            .relative()
            .w(px(560.))
            .max_w_full()
            .h((window.viewport_size().height * 0.8).min(px(720.)))
            .flex()
            .flex_col()
            .overflow_hidden()
            .rounded(px(12.))
            .border_1()
            .border_color(content(0.1))
            .text_color(content(1.))
            .shadow_lg()
            .child(super::surface::backdrop(12.))
            .child(
                div()
                    .relative()
                    .px(px(20.))
                    .py(px(16.))
                    .border_b_1()
                    .border_color(content(0.1))
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("What's new"),
                            )
                            .child(
                                div()
                                    .mt(px(4.))
                                    .text_size(px(12.))
                                    .text_color(content(0.6))
                                    .child(description),
                            ),
                    )
                    .child(
                        div()
                            .id("close-whats-new")
                            .role(Role::Button)
                            .aria_label("Close")
                            .tooltip(chrome_tip("Close", None))
                            .cursor_pointer()
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(WhatsNewEvent::Close)))
                            .child(icon("x", 14.)),
                    ),
            )
            .child(body.relative())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_notice_requires_the_running_version_and_is_consumed_once() {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        store
            .save_setting(MARKER, r#"{"version":"99.0.0-preview.1"}"#)
            .unwrap();
        assert!(consume_installed_update(&store).unwrap().is_none());
        store
            .save_setting(
                MARKER,
                &serde_json::json!({"version": super::super::updater::version()}).to_string(),
            )
            .unwrap();
        assert_eq!(
            consume_installed_update(&store).unwrap().as_deref(),
            Some(super::super::updater::version())
        );
        assert!(consume_installed_update(&store).unwrap().is_none());
    }
}
