//! Right-click menus drawn by AppKit on macOS: the system's look, type-ahead,
//! VoiceOver and key equivalents, as Finder's menus have. A menu whose rows
//! all map onto `NSMenuItem`s (actions, checks, submenus, separators,
//! headers) shows natively; one with custom views, flyouts or switches
//! returns `false` so the caller draws its own [`crate::popover::Menu`].
//! Other platforms and tests always draw their own.
//!
//! The approach follows NativeGPUI (MIT, github.com/Cafekery/NativeGPUI,
//! `src/controls/menu.rs`): AppKit's tracking loop runs from a spawned task,
//! outside GPUI's `App` borrow, and a small target object records the tag of
//! the item picked.
use crate::popover::{MenuAction, MenuItem};
use gpui::{App, Pixels, Point, SharedString, Window};

/// What a menu becomes in AppKit, with each action's id at its tag.
#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
#[derive(Debug, PartialEq)]
struct Plan {
    rows: Vec<Row>,
    ids: Vec<SharedString>,
}

#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
#[derive(Debug, PartialEq)]
enum Row {
    Separator,
    /// A caption or footnote: shown, never picked.
    Text(String),
    Action {
        tag: usize,
        label: String,
        keys: Option<Keys>,
        enabled: bool,
        checked: bool,
        submenu: Vec<Row>,
    },
}

/// A key equivalent as AppKit takes it: the key's characters and its
/// modifiers. Context menus only display it; the app's own bindings run it.
#[cfg_attr(not(all(target_os = "macos", not(test))), allow(dead_code))]
#[derive(Debug, PartialEq, Default)]
struct Keys {
    key: String,
    command: bool,
    shift: bool,
    option: bool,
    control: bool,
}

/// The menu as AppKit rows, or `None` when a row needs GPUI to draw it.
fn plan(items: &[MenuItem]) -> Option<Plan> {
    let mut ids = Vec::new();
    let mut rows = Vec::new();
    for item in items {
        match item {
            MenuItem::Separator => rows.push(Row::Separator),
            MenuItem::Header(text) | MenuItem::Note(text) => rows.push(Row::Text(text.to_string())),
            MenuItem::Action(action) => rows.push(plan_action(action, &mut ids)?),
            MenuItem::Custom(..) => return None,
        }
    }
    // AppKit would draw these as stray lines.
    while matches!(rows.last(), Some(Row::Separator)) {
        rows.pop();
    }
    rows.dedup_by(|a, b| matches!((a, b), (Row::Separator, Row::Separator)));
    (!ids.is_empty()).then_some(Plan { rows, ids })
}

fn plan_action(action: &MenuAction, ids: &mut Vec<SharedString>) -> Option<Row> {
    // A flyout is a GPUI view, a switch updates the menu in place, and a
    // keep-open row expects the menu to stay up: none of these exist in an
    // NSMenu.
    if action.flyout.is_some() || action.switch || action.keep_open {
        return None;
    }
    let submenu = action
        .submenu
        .iter()
        .map(|sub| plan_action(sub, ids))
        .collect::<Option<Vec<_>>>()?;
    let tag = ids.len();
    ids.push(action.id.clone());
    Some(Row::Action {
        tag,
        label: action.label.to_string(),
        keys: action.shortcut.as_deref().and_then(key_equivalent),
        enabled: !action.disabled,
        checked: action.checked,
        submenu,
    })
}

/// Reads a shortcut as the menus label it on macOS ("⌘⇧C", "F2", "⌫") into
/// AppKit's key equivalent. `None` for a label AppKit cannot show.
fn key_equivalent(label: &str) -> Option<Keys> {
    let mut keys = Keys::default();
    let mut rest = label.trim();
    loop {
        let mut chars = rest.chars();
        let flag = match chars.next() {
            Some('⌘') => &mut keys.command,
            Some('⇧') => &mut keys.shift,
            Some('⌥') => &mut keys.option,
            Some('⌃') => &mut keys.control,
            _ => break,
        };
        *flag = true;
        rest = chars.as_str();
    }
    let function = |code: u32| char::from_u32(code).map(String::from);
    keys.key = match rest {
        "" => return None,
        "⌫" => "\u{8}".into(),
        "⌦" => "\u{7f}".into(),
        "↩" | "⏎" | "Enter" | "Return" => "\r".into(),
        "⎋" | "Esc" | "Escape" => "\u{1b}".into(),
        "⇥" | "Tab" => "\t".into(),
        "Space" => " ".into(),
        "↑" => function(0xF700)?,
        "↓" => function(0xF701)?,
        "←" => function(0xF702)?,
        "→" => function(0xF703)?,
        f if f.len() > 1 && f.starts_with('F') => {
            let n: u32 = f[1..].parse().ok().filter(|n| (1..=20).contains(n))?;
            // NSF1FunctionKey is 0xF704.
            function(0xF703 + n)?
        }
        key if key.chars().count() == 1 => key.to_lowercase(),
        _ => return None,
    };
    Some(keys)
}

/// Shows `items` as a native menu where the pointer is, when every row maps
/// onto AppKit. Returns whether it did; on `false` the caller draws its own
/// menu. `done` gets the picked action's id, or `None` when the menu closed
/// without a pick.
pub(crate) fn present(
    items: &[MenuItem],
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(Option<SharedString>, &mut Window, &mut App) + 'static,
) -> bool {
    match plan(items) {
        Some(plan) => show(plan, position, window, cx, done),
        None => false,
    }
}

#[cfg(not(all(target_os = "macos", not(test))))]
fn show(
    _: Plan,
    _: Point<Pixels>,
    _: &mut Window,
    _: &mut App,
    _: impl FnOnce(Option<SharedString>, &mut Window, &mut App) + 'static,
) -> bool {
    false
}

#[cfg(all(target_os = "macos", not(test)))]
fn show(
    plan: Plan,
    position: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(Option<SharedString>, &mut Window, &mut App) + 'static,
) -> bool {
    appkit::show(plan, position, window, cx, done)
}

#[cfg(all(target_os = "macos", not(test)))]
mod appkit {
    use super::{Plan, Row};
    use gpui::{App, Pixels, Point, SharedString, Window};
    use objc2::{
        DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, rc::Retained,
        runtime::NSObject, sel,
    };
    use objc2_app_kit::{NSControlStateValueOn, NSEventModifierFlags, NSMenu, NSMenuItem, NSView};
    use objc2_foundation::{NSPoint, NSString};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use std::cell::Cell;

    struct SinkIvars {
        picked: Cell<isize>,
    }
    define_class!(
        /// Records the tag of the picked item. Menu items hold their target
        /// weakly, so the caller keeps this alive while the menu is up.
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "MonocodeContextMenuSink"]
        #[ivars = SinkIvars]
        struct Sink;

        impl Sink {
            #[unsafe(method(monocodeMenuPick:))]
            fn pick(&self, sender: &NSMenuItem) {
                self.ivars().picked.set(sender.tag());
            }
        }
    );
    impl Sink {
        fn new(mtm: MainThreadMarker) -> Retained<Self> {
            let this = Self::alloc(mtm).set_ivars(SinkIvars {
                picked: Cell::new(-1),
            });
            unsafe { msg_send![super(this), init] }
        }
    }

    thread_local! {
        /// AppKit runs one menu at a time; a second right-click while one is
        /// up belongs to that menu's tracking.
        static SHOWING: Cell<bool> = const { Cell::new(false) };
    }

    fn fill(menu: &NSMenu, rows: &[Row], sink: &Sink, mtm: MainThreadMarker) {
        menu.setAutoenablesItems(false);
        for row in rows {
            let item = match row {
                Row::Separator => NSMenuItem::separatorItem(mtm),
                Row::Text(text) => {
                    let item = NSMenuItem::new(mtm);
                    item.setTitle(&NSString::from_str(text));
                    item.setEnabled(false);
                    item
                }
                Row::Action {
                    tag,
                    label,
                    keys,
                    enabled,
                    checked,
                    submenu,
                } => {
                    let item = NSMenuItem::new(mtm);
                    item.setTitle(&NSString::from_str(label));
                    item.setEnabled(*enabled);
                    item.setTag(*tag as isize);
                    if *checked {
                        item.setState(NSControlStateValueOn);
                    }
                    if submenu.is_empty() {
                        unsafe {
                            item.setTarget(Some(sink));
                            item.setAction(Some(sel!(monocodeMenuPick:)));
                        }
                    } else {
                        let sub =
                            NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(label));
                        fill(&sub, submenu, sink, mtm);
                        item.setSubmenu(Some(&sub));
                    }
                    if let Some(keys) = keys {
                        let mut mask = NSEventModifierFlags::empty();
                        for (on, flag) in [
                            (keys.command, NSEventModifierFlags::Command),
                            (keys.shift, NSEventModifierFlags::Shift),
                            (keys.option, NSEventModifierFlags::Option),
                            (keys.control, NSEventModifierFlags::Control),
                        ] {
                            if on {
                                mask |= flag;
                            }
                        }
                        item.setKeyEquivalent(&NSString::from_str(&keys.key));
                        item.setKeyEquivalentModifierMask(mask);
                    }
                    item
                }
            };
            menu.addItem(&item);
        }
    }

    fn content_view(window: &Window) -> Option<Retained<NSView>> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return None;
        };
        // SAFETY: GPUI's AppKit handle points at its live content view.
        unsafe { Retained::retain(appkit.ns_view.as_ptr().cast::<NSView>()) }
    }

    pub(super) fn show(
        plan: Plan,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut App,
        done: impl FnOnce(Option<SharedString>, &mut Window, &mut App) + 'static,
    ) -> bool {
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(view) = content_view(window) else {
            return false;
        };
        if SHOWING.get() {
            return true;
        }
        SHOWING.set(true);
        window
            .spawn(cx, async move |cx| {
                let menu = NSMenu::new(mtm);
                let sink = Sink::new(mtm);
                fill(&menu, &plan.rows, &sink, mtm);
                // Runs AppKit's tracking loop until the menu closes. No GPUI
                // borrow is held here, so frames keep drawing meanwhile.
                // Placed where the pointer is. `popUpContextMenu:withEvent:`
                // would add AutoFill and Services, since GPUI's view is a
                // text input client; these menus act on tabs and files.
                let height = view.bounds().size.height;
                let (x, y) = (f64::from(position.x), f64::from(position.y));
                let at = NSPoint::new(x, if view.isFlipped() { y } else { height - y });
                menu.popUpMenuPositioningItem_atLocation_inView(None, at, Some(&view));
                SHOWING.set(false);
                let picked = usize::try_from(sink.ivars().picked.get())
                    .ok()
                    .and_then(|tag| plan.ids.get(tag).cloned());
                let _ = cx.update(|window, cx| done(picked, window, cx));
            })
            .detach();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::popover::{Flyout, MenuAction};
    use gpui::AppContext as _;

    fn keys(key: &str) -> Keys {
        Keys {
            key: key.into(),
            ..Default::default()
        }
    }

    #[test]
    fn shortcut_labels_become_appkit_key_equivalents() {
        assert_eq!(
            key_equivalent("⌘⇧C"),
            Some(Keys {
                command: true,
                shift: true,
                ..keys("c")
            })
        );
        assert_eq!(key_equivalent("F2"), Some(keys("\u{F705}")));
        assert_eq!(key_equivalent("⌫"), Some(keys("\u{8}")));
        assert_eq!(
            key_equivalent("⌥↑"),
            Some(Keys {
                option: true,
                ..keys("\u{F700}")
            })
        );
        // Labels AppKit cannot show stay off the item, not mangled.
        assert_eq!(key_equivalent("Ctrl+K"), None);
        assert_eq!(key_equivalent("⌘"), None);
        assert_eq!(key_equivalent("F99"), None);
    }

    #[test]
    fn plain_menus_map_onto_appkit_rows_with_ids_at_their_tags() {
        let items: Vec<MenuItem> = vec![
            MenuAction::new("copy", "Copy").shortcut("⌘C").into(),
            MenuItem::Separator,
            MenuItem::Separator,
            MenuAction::new("sort", "Sort")
                .submenu(vec![
                    MenuAction::new("name", "Name").checked(true),
                    MenuAction::new("date", "Date"),
                ])
                .into(),
            MenuAction::new("delete", "Delete")
                .danger()
                .disabled(true)
                .into(),
            MenuItem::Separator,
        ];
        let plan = plan(&items).expect("every row maps onto AppKit");
        let ids: Vec<&str> = plan.ids.iter().map(|id| id.as_ref()).collect();
        assert_eq!(ids, ["copy", "name", "date", "sort", "delete"]);
        // Doubled and trailing separators are dropped.
        assert_eq!(plan.rows.len(), 4);
        let Row::Action { submenu, tag, .. } = &plan.rows[2] else {
            panic!("the submenu row");
        };
        assert_eq!(*tag, 3);
        assert!(matches!(
            submenu[0],
            Row::Action {
                tag: 1,
                checked: true,
                ..
            }
        ));
        assert!(matches!(plan.rows[3], Row::Action { enabled: false, .. }));
    }

    #[gpui::test]
    fn rows_only_gpui_can_draw_keep_the_drawn_menu(cx: &mut gpui::TestAppContext) {
        let view = cx.update(|cx| cx.new(|_| gpui::Empty)).into();
        let flyout = Flyout {
            view,
            width: 200.,
            padding: 0.,
            height: None,
            max_height: None,
            focus: None,
            surface: None,
        };
        let with = |action: MenuAction| plan(&[MenuAction::new("a", "A").into(), action.into()]);
        assert!(with(MenuAction::new("b", "B").flyout(flyout)).is_none());
        assert!(with(MenuAction::new("b", "B").toggle(true)).is_none());
        assert!(with(MenuAction::new("b", "B").keep_open()).is_none());
        assert!(with(MenuAction::new("b", "B")).is_some());
        // A menu of captions alone has nothing to pick.
        assert!(plan(&[MenuItem::Header("Nothing".into())]).is_none());
    }
}
