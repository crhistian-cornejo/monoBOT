//! Windows has no menu bar, so the title bar's left end holds a menu button,
//! as Windows apps with their own chrome do. It lists the app menu
//! (`app_menu.rs`) one submenu per top-level menu; rows dispatch the same
//! actions, show their live shortcuts and grey out where the focused view
//! cannot run them.
use super::*;

/// The leading room the button takes, where macOS keeps its traffic lights:
/// a 10 px inset, the 24 px button and a 4 px gap before the sidebar toggle.
pub(super) const MENU_BUTTON_SLOT: f32 = 10. + 24. + 4.;

impl Shell {
    pub(super) fn app_menu_button(&self, cx: &mut Context<Self>) -> gpui::Div {
        let open = self.menu_open(&MenuTarget::AppMenu);
        div()
            .w(px(MENU_BUTTON_SLOT))
            .h_full()
            .flex_shrink_0()
            .flex()
            .items_center()
            .child(titlebar_drag_area().w(px(10.)).flex_shrink_0())
            .child(
                icon_button("app-menu", "Menu", "menu")
                    .aria_expanded(open)
                    .relative()
                    .when(open, |d| d.bg(content(0.1)).text_color(content(1.)))
                    .child(self.app_menu_anchor.probe())
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, window, cx| {
                        if this.menu_open(&MenuTarget::AppMenu) {
                            this.close_menu(cx);
                        } else {
                            let anchor = Anchor::Slot(this.app_menu_anchor.clone());
                            this.open_menu(MenuTarget::AppMenu, anchor, window, cx);
                        }
                    })),
            )
            .child(titlebar_drag_area().flex_1())
    }

    /// One row per top-level menu, each opening its items. Read before the
    /// menu takes focus, so availability reflects the view the user was in.
    pub(super) fn app_menu_items(&self, window: &Window, cx: &App) -> Vec<MenuItem> {
        crate::app_menu::owned(&self.workspace.preferences)
            .iter()
            .enumerate()
            .map(|(menu_ix, menu)| {
                let items = menu
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(item_ix, item)| {
                        let gpui::OwnedMenuItem::Action {
                            name,
                            action,
                            checked,
                            disabled,
                            ..
                        } = item
                        else {
                            return None;
                        };
                        let row = MenuAction::new(
                            format!("{menu_ix}:{item_ix}"),
                            SharedString::from(name.clone()),
                        )
                        .checked(*checked)
                        .disabled(*disabled || !window.is_action_available(&**action, cx));
                        Some(match shortcut(window, &**action) {
                            Some(keys) => row.shortcut(keys),
                            None => row,
                        })
                    })
                    .collect();
                MenuAction::new(format!("{menu_ix}"), menu.name.clone())
                    .submenu(items)
                    .into()
            })
            .collect()
    }

    /// Runs a picked `menu:item` row. The menu hands focus back first, so the
    /// action reaches the view it was picked for.
    pub(super) fn run_app_menu(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some((menu_ix, item_ix)) = id
            .split_once(':')
            .and_then(|(m, i)| Some((m.parse::<usize>().ok()?, i.parse::<usize>().ok()?)))
        else {
            return;
        };
        let menus = crate::app_menu::owned(&self.workspace.preferences);
        let Some(action) = menus
            .get(menu_ix)
            .and_then(|menu| match menu.items.get(item_ix)? {
                gpui::OwnedMenuItem::Action { action, .. } => Some(action.boxed_clone()),
                _ => None,
            })
        else {
            return;
        };
        window.defer(cx, move |window, cx| window.dispatch_action(action, cx));
    }
}

fn shortcut(window: &Window, action: &dyn gpui::Action) -> Option<SharedString> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    let keys: Vec<String> = binding
        .keystrokes()
        .iter()
        .map(|keystroke| crate::settings::shortcut_label(&keystroke.unparse()))
        .collect();
    Some(keys.join(" ").into())
}
