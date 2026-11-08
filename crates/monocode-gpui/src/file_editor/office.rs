//! Office files in the file pane: spreadsheets, Word documents and
//! PowerPoint decks, read in the background by the core and shown by the
//! views beside this module. Read-only; the system's app is a click away.
use super::*;
use gpui::{AnyView, ImageSource, Stateful};
use monocode_core::office::{self, Deck, DocxDocument, Workbook};
use std::{path::Path, time::SystemTime};

/// What a path opens as, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Sheet,
    Word,
    Deck,
}

pub(super) fn extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or("");
    match name.rfind('.') {
        Some(dot) if dot > 0 => name[dot + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// Binary workbooks, Word documents and decks open here. `.csv` and `.tsv`
/// stay text in the editor, with a sheet as their preview.
pub(super) fn kind_of(path: &str) -> Option<Kind> {
    if path.starts_with("remote://") {
        return None;
    }
    match extension(path).as_str() {
        "xlsx" | "xlsm" | "xls" => Some(Kind::Sheet),
        "docx" | "docm" => Some(Kind::Word),
        "pptx" | "pptm" | "ppsx" => Some(Kind::Deck),
        _ => None,
    }
}

/// Whether a text file's preview is a sheet.
pub(super) fn is_delimited(path: &str) -> bool {
    matches!(extension(path).as_str(), "csv" | "tsv")
}

pub(super) enum Loaded {
    Sheet(Workbook),
    Word(Box<DocxDocument>),
    Deck(Box<Deck>),
}

/// Size and modification time: a file that has not changed is not read again.
pub(super) type Stamp = (u64, Option<SystemTime>);

pub(super) fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

pub(super) fn read(path: &Path, kind: Kind) -> Result<(Loaded, Option<Stamp>), String> {
    let stamp = stamp(path);
    let loaded = match kind {
        Kind::Sheet => Loaded::Sheet(office::read_workbook(path)?),
        Kind::Word => Loaded::Word(Box::new(office::read_docx(path)?)),
        Kind::Deck => Loaded::Deck(Box::new(office::read_pptx(path)?)),
    };
    Ok((loaded, stamp))
}

/// The view for what was read.
pub(super) fn view(loaded: Loaded, path: &str, cx: &mut Context<FileEditor>) -> AnyView {
    match loaded {
        Loaded::Sheet(workbook) => {
            AnyView::from(cx.new(|cx| super::spreadsheet::SpreadsheetView::new(path, workbook, cx)))
        }
        Loaded::Word(document) => {
            AnyView::from(cx.new(|cx| super::word::DocumentView::new(path, *document, cx)))
        }
        Loaded::Deck(deck) => {
            AnyView::from(cx.new(|cx| super::deck::DeckView::new(path, *deck, cx)))
        }
    }
}

/// The picture format bytes are in, by their first bytes. Pictures are
/// sniffed, never trusted by name.
pub(super) fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else {
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(512)]);
        head.contains("<svg").then_some(ImageFormat::Svg)
    }
}

/// A MIME type the viewer can draw, or the bytes' own format.
pub(super) fn format_of(mime: &str, bytes: &[u8]) -> Option<ImageFormat> {
    sniff(bytes).or(match mime {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" | "image/jpg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::Webp),
        "image/bmp" => Some(ImageFormat::Bmp),
        "image/svg+xml" => Some(ImageFormat::Svg),
        _ => None,
    })
}

/// Pictures decoded off the UI thread, each given back when the view closes.
pub(super) struct Pictures {
    slots: Vec<Option<Arc<RenderImage>>>,
}

impl Pictures {
    pub(super) fn new(count: usize) -> Self {
        Self {
            slots: vec![None; count],
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<Arc<RenderImage>> {
        self.slots.get(index).cloned().flatten()
    }

    pub(super) fn set(&mut self, index: usize, image: Option<Arc<RenderImage>>) {
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = image;
        }
    }

    /// Give the sprites back; GPUI keeps them for the life of the app else.
    pub(super) fn release(&mut self, cx: &mut App) {
        for slot in &mut self.slots {
            if let Some(image) = slot.take() {
                cx.drop_image(image, None);
            }
        }
    }
}

/// Decode pictures in the background, handing each to `apply` as it lands.
pub(super) fn decode<V: 'static>(
    sources: Vec<(usize, Vec<u8>, ImageFormat)>,
    apply: fn(&mut V, usize, Option<Arc<RenderImage>>),
    cx: &mut Context<V>,
) {
    for (index, bytes, format) in sources {
        let renderer = cx.svg_renderer();
        let work = cx.background_executor().spawn(async move {
            Image::from_bytes(format, bytes)
                .to_image_data(renderer)
                .ok()
        });
        cx.spawn(async move |this, cx| {
            let image = work.await;
            let _ = this.update(cx, |this, cx| {
                apply(this, index, image);
                cx.notify();
            });
        })
        .detach();
    }
}

/// The same picture as a data URL for the lightbox.
pub(super) fn lightbox(
    bytes: &[u8],
    format: ImageFormat,
    alt: &str,
) -> crate::image_lightbox::Open {
    use base64::Engine;
    crate::image_lightbox::Open {
        source: crate::image_lightbox::Source::Embedded {
            data: base64::engine::general_purpose::STANDARD
                .encode(bytes)
                .into(),
            mime: format.mime_type().into(),
        },
        alt: alt.to_string().into(),
    }
}

pub(super) fn picture(image: Arc<RenderImage>) -> gpui::Img {
    img(ImageSource::Render(image))
}

/// Open the file in the system's app for it.
pub(super) fn open_externally(path: &str, cx: &mut App) {
    cx.open_url(&format!(
        "file://{}",
        path.replace('%', "%25")
            .replace(' ', "%20")
            .replace('#', "%23")
            .replace('?', "%3F")
    ));
}

pub(super) fn reveal(path: &str, cx: &mut App) {
    let path = PathBuf::from(path);
    cx.background_executor()
        .spawn(async move {
            let _ = monocode_core::files::reveal_path(&path);
        })
        .detach();
}

/// A small icon button for the bars above a document.
pub(super) fn bar_button(
    id: &'static str,
    label: &'static str,
    icon_name: &'static str,
) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .role(Role::Button)
        .aria_label(label)
        .tooltip(crate::theme::chrome_tip(label, None))
        .cursor_pointer()
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .hover(|s| s.bg(content(0.08)))
        .child(icon(icon_name, 14.))
}

/// The bar above a document or deck: how many pages or slides, then the two
/// actions.
pub(super) fn count_bar<V: 'static>(
    path: &str,
    count: usize,
    nouns: (&str, &str),
    cx: &mut Context<V>,
) -> AnyElement {
    div()
        .flex_shrink_0()
        .h(px(32.))
        .flex()
        .items_center()
        .gap_1()
        .px_3()
        .border_b_1()
        .border_color(crate::right_panel::stroke())
        .text_size(px(12.))
        .text_color(content(0.5))
        .child(
            div()
                .id("office-count")
                .debug_selector(|| "office-count".into())
                .flex_1()
                .min_w_0()
                .truncate()
                .font_features(crate::right_panel::tabular())
                .child(format!(
                    "{count} {}",
                    if count == 1 { nouns.0 } else { nouns.1 }
                )),
        )
        .child(bar_actions(path, cx))
        .into_any_element()
}

/// Where a picture's bytes live, so a render finds the decode made for them.
pub(super) fn identity(bytes: &[u8]) -> usize {
    bytes.as_ptr() as usize
}

/// The two buttons every bar ends with: open in the app, reveal in the folder.
pub(super) fn bar_actions<V: 'static>(path: &str, cx: &mut Context<V>) -> gpui::Div {
    let open_path = path.to_string();
    let reveal_path = path.to_string();
    div()
        .flex()
        .items_center()
        .gap_0p5()
        .flex_shrink_0()
        .child(
            bar_button("office-open", "Open in default app", "external-link")
                .on_click(cx.listener(move |_, _, _, cx| open_externally(&open_path, cx))),
        )
        .child(
            bar_button("office-reveal", "Reveal in folder", "folder")
                .on_click(cx.listener(move |_, _, _, cx| reveal(&reveal_path, cx))),
        )
}
