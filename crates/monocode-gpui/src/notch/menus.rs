//! The notch's right-click menus, drawn by AppKit on macOS as the
//! workspace's are (`crate::native_menu`, after NativeGPUI): the system's
//! look, type-ahead and VoiceOver. A copy in the Clipboard page offers what
//! its card does and more (copy it written another way, find the copies
//! that share a tag, take it out); a widget on the shelf can be narrowed,
//! widened, moved or taken out without the pencil. Elsewhere the rows keep
//! their own buttons and a right-click does nothing.
//!
//! The menu runs AppKit's tracking loop while the pointer may be off the
//! island, so the notch stays open until it closes (`menu_up`).
use super::*;
use crate::popover::{MenuAction, MenuItem};
use gpui::{Point, SharedString};
use monocode_core::notch::Widget;
use monocode_core::notch::clipboard::{self as clipboard, Clip, Kind};

/// What a copy's menu offers, by the ids `clip_action` runs.
pub(super) fn clip_items(clip: &Clip) -> Vec<MenuItem> {
    let mut items: Vec<MenuItem> = vec![MenuAction::new("copy", "Copy").into()];
    match clip.kind {
        Kind::Color(color) => {
            let formats = clipboard::color_formats(color)
                .into_iter()
                .enumerate()
                .map(|(at, written)| MenuAction::new(format!("format-{at}"), written))
                .collect();
            let mut copy_as = MenuAction::new("copy-as", "Copy As");
            copy_as.submenu = formats;
            items.push(copy_as.into());
        }
        Kind::Link => items.push(MenuAction::new("open", "Open Link").into()),
        Kind::Email => items.push(MenuAction::new("write", "Write Email").into()),
        Kind::Path => items.push(MenuAction::new("show", REVEAL).into()),
        Kind::Text | Kind::Code | Kind::Image(..) => {}
    }
    items.push(MenuItem::Separator);
    let mut tagged = MenuAction::new("tagged", "Show Copies Tagged");
    tagged.submenu = clip
        .tags()
        .into_iter()
        .map(|tag| MenuAction::new(format!("tag:{tag}"), format!("#{tag}")))
        .collect();
    items.push(tagged.into());
    items.push(MenuItem::Separator);
    items.push(MenuAction::new("remove", "Remove from History").into());
    items.push(MenuAction::new("clear", "Clear History").danger().into());
    items
}

/// Finder's name for showing a file where it is, on the Mac.
const REVEAL: &str = if cfg!(target_os = "macos") {
    "Show in Finder"
} else {
    "Show in Folder"
};

/// What a widget's menu offers on the shelf as it is now, by the ids
/// `widget_action` runs.
pub(super) fn widget_items(look: &Look, widget: &Widget) -> Vec<MenuItem> {
    let id = widget.id.as_str();
    let can = |change: &dyn Fn(&mut Look) -> bool| change(&mut look.clone());
    let at = look.shelf.iter().position(|kept| kept.id == widget.id);
    vec![
        MenuAction::new("narrower", "Narrower")
            .disabled(!can(&|look| look.resize(id, -1)))
            .into(),
        MenuAction::new("wider", "Wider")
            .disabled(!can(&|look| look.resize(id, 1)))
            .into(),
        MenuItem::Separator,
        MenuAction::new("earlier", "Move Earlier")
            .disabled(at == Some(0) || !can(&|look| look.shift(id, -1)))
            .into(),
        MenuAction::new("later", "Move Later")
            .disabled(!can(&|look| look.shift(id, 1)))
            .into(),
        MenuItem::Separator,
        MenuAction::new("arrange", "Arrange Widgets…").into(),
        MenuAction::new("remove", "Remove Widget").danger().into(),
    ]
}

impl Notch {
    /// Shows `items` as AppKit's menu where the pointer is and runs the
    /// pick with `run`. The notch stays open while the menu is up. False
    /// where there is no native menu (other systems, tests).
    fn show_menu(
        &mut self,
        items: Vec<MenuItem>,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
        run: impl FnOnce(&mut Self, SharedString, &mut Window, &mut Context<Self>) + 'static,
    ) -> bool {
        let this = cx.entity().downgrade();
        self.menu_up = true;
        let shown =
            crate::native_menu::present(&items, at, window, cx, move |picked, window, cx| {
                let _ = this.update(cx, |this, cx| {
                    this.menu_closed(cx);
                    if let Some(id) = picked {
                        run(this, id, window, cx);
                    }
                });
            });
        if !shown {
            self.menu_up = false;
        }
        shown
    }

    /// The menu is gone: if the pointer left the island meanwhile, the
    /// notch closes as it would have.
    pub(super) fn menu_closed(&mut self, cx: &mut Context<Self>) {
        self.menu_up = false;
        if self.open && !self.pointer_over() {
            self.hover = None;
            self.hover_changed(false, cx);
        }
    }

    /// A copy was right-clicked.
    pub(super) fn clip_menu(
        &mut self,
        index: usize,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(clip) = self.clips.clips().get(index) else {
            return;
        };
        let items = clip_items(clip);
        // The menu names the copy by when it was made: a copy coming in
        // while it is up moves the rows down.
        let made = clip.at;
        self.show_menu(items, at, window, cx, move |this, id, _, cx| {
            if let Some(index) = this.clips.clips().iter().position(|clip| clip.at == made) {
                this.clip_action(index, &id, cx);
            }
        });
    }

    /// Runs what was picked in a copy's menu.
    pub(super) fn clip_action(&mut self, index: usize, id: &str, cx: &mut Context<Self>) {
        let Some(clip) = self.clips.clips().get(index).cloned() else {
            return;
        };
        let text = clip.text.trim().to_owned();
        match id {
            "copy" => self.copy_clip(index, cx),
            "open" => cx.open_url(&text),
            "write" => cx.open_url(&format!("mailto:{text}")),
            "show" => cx.reveal_path(std::path::Path::new(&text)),
            "remove" => {
                self.clips.remove(index);
                self.clip_copied = None;
                cx.notify();
            }
            "clear" => self.clear_clips(cx),
            id => {
                if let (Some(at), Kind::Color(color)) = (id.strip_prefix("format-"), clip.kind)
                    && let Some(written) = at
                        .parse::<usize>()
                        .ok()
                        .and_then(|at| clipboard::color_formats(color).get(at).cloned())
                {
                    self.copy_text(written, cx);
                } else if let Some(tag) = id.strip_prefix("tag:") {
                    self.search_clips(format!("#{tag}"), cx);
                }
            }
        }
    }

    /// A widget on the shelf was right-clicked.
    pub(super) fn widget_menu(
        &mut self,
        widget: Widget,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let items = widget_items(&self.look, &widget);
        self.show_menu(items, at, window, cx, move |this, id, _, cx| {
            this.widget_action(&widget.id, &id, cx);
        });
    }

    /// Runs what was picked in a widget's menu.
    pub(super) fn widget_action(&mut self, widget: &str, id: &str, cx: &mut Context<Self>) {
        if id == "arrange" {
            self.editing = true;
            cx.notify();
            return;
        }
        let widget = widget.to_owned();
        let id = id.to_owned();
        self.change_look(
            move |look| {
                match id.as_str() {
                    "narrower" => look.resize(&widget, -1),
                    "wider" => look.resize(&widget, 1),
                    "earlier" => look.shift(&widget, -1),
                    "later" => look.shift(&widget, 1),
                    "remove" => {
                        look.remove(&widget);
                        true
                    }
                    _ => false,
                };
            },
            cx,
        );
    }
}
