//! Cited sites' own icons, shared by every list of sources. A site is asked
//! for its icon once per run; until it answers, and for a site without one, the
//! badge shows the host's letter (CitationMonogram).
use crate::theme::{background_base, content};
use gpui::{AnyElement, App, Image, ImageFormat, ObjectFit, div, img, prelude::*, px};
use monocode_core::citations;
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

/// `None` while a site is being asked, and for good when it has no icon.
static ICONS: LazyLock<Mutex<HashMap<String, Option<Arc<Image>>>>> =
    LazyLock::new(Default::default);

/// The site's icon when it is known; the first call for a host starts fetching
/// it and repaints the windows once it arrives.
fn icon(host: &str, url: &str, cx: &mut App) -> Option<Arc<Image>> {
    {
        let mut icons = ICONS.lock().ok()?;
        if let Some(known) = icons.get(host) {
            return known.clone();
        }
        icons.insert(host.to_owned(), None);
    }
    if cfg!(test) {
        return None;
    }
    let host = host.to_owned();
    let url = url.to_owned();
    let work = cx
        .background_executor()
        .spawn(async move { monocode_core::web::site_icon_png(&url) });
    cx.spawn(async move |cx| {
        if let Some(png) = work.await {
            if let Ok(mut icons) = ICONS.lock() {
                icons.insert(
                    host,
                    Some(Arc::new(Image::from_bytes(ImageFormat::Png, png))),
                );
            }
            cx.update(|cx| cx.refresh_windows());
        }
    })
    .detach();
    None
}

/// A round badge of `size` points for a cited site.
pub(crate) fn badge(host: &str, url: &str, size: f32, cx: &mut App) -> AnyElement {
    let frame = div()
        .size(px(size))
        .flex_shrink_0()
        .rounded_full()
        .border_1()
        .border_color(background_base())
        .overflow_hidden()
        .bg(content(0.12));
    match icon(host, url, cx) {
        Some(icon) => frame
            .child(img(icon).size_full().object_fit(ObjectFit::Cover))
            .into_any_element(),
        None => frame
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(size * 0.5))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(content(0.7))
            .child(citations::monogram(host))
            .into_any_element(),
    }
}
