//! The notch's file tray: drag files or folders onto the notch and they
//! wait on Home, above the shelf, until they are opened, shown in their
//! folder or taken out. Only their paths are kept, in memory
//! (`monocode_core::notch::tray`); the files are never touched.
use super::*;
use monocode_core::notch::tray as tray_model;
use std::path::PathBuf;

/// The tray's row on Home.
pub(super) const TRAY_HEIGHT: f32 = 26.;

/// One of a file's two small buttons.
fn chip_tool(
    id: gpui::SharedString,
    label: &'static str,
    path: &'static str,
) -> gpui::Stateful<gpui::Div> {
    let selector = id.clone();
    div()
        .id(id)
        .debug_selector(move || selector.to_string())
        .role(Role::Button)
        .aria_label(label)
        .size(px(16.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(|style| style.bg(ink(0.2)))
        .child(icon(path, 10.).text_color(ink(0.7)))
}

impl Notch {
    /// Files are held over the notch: it opens on Home to take them.
    pub(super) fn files_over(&mut self, cx: &mut Context<Self>) {
        if self.dropping && self.open {
            return;
        }
        self.dropping = true;
        self.tab = Tab::Home;
        self.editing = false;
        self.set_open(true, cx);
        cx.notify();
    }

    /// What was dropped on the notch goes to the front of its tray.
    pub(crate) fn drop_files(&mut self, dropped: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.dropping = false;
        self.tray.add(dropped);
        self.tab = Tab::Home;
        self.set_open(true, cx);
        cx.notify();
    }

    fn tray_shown(&self) -> bool {
        !self.tray.is_empty() || self.dropping
    }

    /// The room the tray takes on Home, with the gap under it.
    pub(super) fn tray_height(&self) -> f32 {
        if self.tray_shown() {
            TRAY_HEIGHT + model::CELL_GAP
        } else {
            0.
        }
    }

    pub(super) fn tray_view(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.tray_shown() {
            return div().into_any_element();
        }
        let mut files = div()
            .id("notch-tray-files")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .items_center()
            .gap(px(6.))
            .overflow_x_scroll()
            .track_scroll(&self.tray_scroll);
        if self.tray.is_empty() {
            files = files.child(
                div()
                    .text_size(px(11.))
                    .text_color(ink(0.5))
                    .child("Drop to keep it here"),
            );
        }
        for (index, path) in self.tray.items().iter().enumerate() {
            let key = |name: &str| gpui::SharedString::from(format!("notch-tray-{name}-{index}"));
            // A name without an extension reads as a folder: nothing is
            // asked of the disk to draw the tray.
            let folder = path.extension().is_none();
            let (open, reveal, remove) = (path.clone(), path.clone(), path.clone());
            let selector = key("file");
            files = files.child(
                div()
                    .id(key("file"))
                    .debug_selector(move || selector.to_string())
                    .role(Role::Button)
                    .aria_label(format!("Open {}", tray_model::name(path)))
                    .h(px(22.))
                    .flex_shrink_0()
                    .pl(px(7.))
                    .pr(px(3.))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .bg(ink(0.1))
                    .cursor_pointer()
                    .hover(|style| style.bg(ink(0.16)))
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&open)))
                    .child(icon(if folder { "folder" } else { "file" }, 11.).text_color(ink(0.6)))
                    .child(
                        div()
                            .max_w(px(130.))
                            .truncate()
                            .text_size(px(11.))
                            .text_color(ink(0.9))
                            .child(tray_model::name(path)),
                    )
                    .child(
                        chip_tool(key("reveal"), "Show in its folder", "folder-tree").on_click(
                            cx.listener(move |_, _, _, cx| {
                                cx.stop_propagation();
                                cx.reveal_path(&reveal);
                            }),
                        ),
                    )
                    .child(
                        chip_tool(key("remove"), "Take out of the tray", "x").on_click(
                            cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                this.tray.remove(&remove);
                                cx.notify();
                            }),
                        ),
                    ),
            );
        }
        div()
            .debug_selector(|| "notch-tray".into())
            .h(px(TRAY_HEIGHT))
            .mb(px(model::CELL_GAP))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(icon("file-add", 12.).text_color(ink(0.45)))
            .child(files)
            .when(!self.tray.is_empty(), |row| {
                row.child(
                    pill("notch-tray-clear", "Clear", false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.tray.clear();
                            cx.notify();
                        },
                    )),
                )
            })
            .into_any_element()
    }
}
