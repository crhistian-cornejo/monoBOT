use crate::settings::SettingsView;
use crate::{
    settings::{
        SettingsEvent,
        management::Target,
        widgets::{icon_action, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, SharedString, div, prelude::*, px};
/// A status line under the buttons: red for an error.
fn status_line(id: &'static str, text: String, error: bool) -> AnyElement {
    div()
        .id(id)
        .debug_selector(move || format!("settings-{id}"))
        .role(if error {
            gpui::Role::Alert
        } else {
            gpui::Role::Status
        })
        .max_w(px(320.))
        .text_size(px(11.))
        .line_height(px(16.))
        .text_align(gpui::TextAlign::Right)
        .text_color(if error {
            gpui::rgb(0xf87171).into()
        } else {
            content(0.5)
        })
        .child(text)
        .into_any_element()
}
impl SettingsView {
    #[cfg(test)]
    pub(crate) fn bots_status(&self) -> Option<&(String, bool)> {
        self.bots_status.as_ref()
    }
    /// The line under Export and Import: what the last one did.
    pub(crate) fn set_bots_status(
        &mut self,
        status: Option<(String, bool)>,
        cx: &mut Context<Self>,
    ) {
        self.bots_status = status;
        cx.notify();
    }
    /// BotTransferControls and BotSyncControls, in their two rows.
    fn bots_group(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let transfer = {
            let mut column = div().flex().flex_col().items_end().gap(px(6.)).child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        secondary_button("bots-export", "Export bots", false, false)
                            .child("Export bots")
                            .debug_selector(|| "settings-bots-export".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bots_status = None;
                                cx.emit(SettingsEvent::BotsExport);
                                cx.notify();
                            })),
                    )
                    .child(
                        secondary_button("bots-import", "Import bots", false, false)
                            .child("Import bots")
                            .debug_selector(|| "settings-bots-import".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.bots_status = None;
                                cx.emit(SettingsEvent::BotsImport);
                                cx.notify();
                            })),
                    ),
            );
            if let Some((text, error)) = self.bots_status.clone() {
                column = column.child(status_line("bots-transfer-status", text, error));
            }
            column.into_any_element()
        };
        let sync = {
            let view = self.ctx.bot_sync.clone();
            let folder = view.folder.clone();
            let mut buttons = div().flex().gap_2();
            if folder.is_some() {
                buttons = buttons
                    .child(
                        secondary_button("bots-sync-now", "Sync now", false, false)
                            .child("Sync now")
                            .debug_selector(|| "settings-bots-sync-now".into())
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::BotSyncNow)),
                            ),
                    )
                    .child(
                        secondary_button("bots-sync-off", "Turn off", false, false)
                            .child("Turn off")
                            .debug_selector(|| "settings-bots-sync-off".into())
                            .on_click(
                                cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::BotSyncOff)),
                            ),
                    );
            }
            buttons = buttons.child(
                secondary_button(
                    "bots-sync-choose",
                    if folder.is_some() {
                        "Change folder"
                    } else {
                        "Choose folder"
                    },
                    false,
                    false,
                )
                .child(if folder.is_some() {
                    "Change folder"
                } else {
                    "Choose folder"
                })
                .debug_selector(|| "settings-bots-sync-choose".into())
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::BotSyncChoose))),
            );
            let mut column = div()
                .flex()
                .flex_col()
                .items_end()
                .gap(px(6.))
                .child(buttons);
            if let Some(folder) = folder {
                column = column.child(status_line("bots-sync-folder", folder, false));
                if let Some(status) = view.status {
                    let at = chrono::DateTime::from_timestamp_millis(status.at as i64)
                        .map(|at| {
                            at.with_timezone(&chrono::Local)
                                .format("%-I:%M %p")
                                .to_string()
                        })
                        .unwrap_or_default();
                    column = column.child(status_line(
                        "bots-sync-status",
                        format!("{} \u{b7} {at}", status.text),
                        status.error == Some(true),
                    ));
                }
            }
            column.into_any_element()
        };
        self.group(
            None,
            false,
            "Bots on another computer",
            None,
            None,
            vec![
                self.row(
                    Some("bots-transfer"),
                    true,
                    "Export and import bots",
                    Some("Save every bot (its instructions, memory, habits, tools, model and permissions) to a file, then import it on your other computer, Windows or Mac. A bot that already exists there is compared with the imported one before you choose which to keep. Folders you opened to a bot stay on each computer.".into()),
                    vec![transfer],
                ),
                self.row(
                    Some("bots-sync"),
                    false,
                    "Keep bots in sync",
                    Some("Choose a folder your computers already share (OneDrive, iCloud Drive, Google Drive, Dropbox). Each computer writes its own file there and reads the others' every couple of minutes: a bot changed on one updates on the other, a new one appears, and one changed on both opens the comparison so you choose.".into()),
                    vec![sync],
                ),
            ],
        )
    }
    pub(super) fn monos_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let mut rows=vec![
            self.row(Some("monos-enabled"),true,"Show monos",Some("Agents of your own on the project rail. Each works on the projects you give it, remembers what matters and picks up habits it runs on its own. Turn this off to hide them.".into()),vec![self.toggle("monos-enabled","Show monos",p.monos,false,cx,|this,on,cx|this.update(cx,|p|p.monos=on))]),
            self.row(Some("rail-monos-pinned"),false,"Pin monos to the icon rail",Some("When the project rail is collapsed to icons, show each Mono at the top of the rail, above a divider, instead of inside the project picker.".into()),vec![self.toggle("rail-monos-pinned","Pin monos to the icon rail",p.rail_monos_pinned,!p.monos,cx,|this,on,cx|this.update(cx,|p|p.rail_monos_pinned=on))])];
        if cfg!(target_os = "macos") {
            rows.push(self.row(Some("mono-menu-bar-icon"),false,"Menu bar icon",Some("Chat with a Mono or open the quick composer from the macOS menu bar. Turn this off to hide the icon.".into()),vec![self.toggle("mono-menu-bar-icon","Menu bar icon",p.mono_menu_bar_icon,false,cx,|this,on,cx|this.update(cx,|p|p.mono_menu_bar_icon=on))]));
        }
        if cfg!(target_os = "windows") {
            // Windows keeps a new tray icon among the hidden ones; only the
            // user can move it to the taskbar.
            let taskbar =
                secondary_button("open-taskbar-settings", "Taskbar settings", false, false)
                    .child("Taskbar settings")
                    .on_click(|_, _, cx| cx.open_url("ms-settings:taskbar"));
            rows.push(self.row(
                Some("mono-menu-bar-icon"),
                false,
                "Tray icon",
                Some("Click monoBOT's icon in the system tray to chat with a Mono or open the quick composer. Windows hides new icons behind the arrow: drag it onto the taskbar, or turn on monoBOT in Taskbar settings › Other system tray icons.".into()),
                vec![
                    taskbar.into_any_element(),
                    self.toggle("mono-menu-bar-icon","Tray icon",p.mono_menu_bar_icon,false,cx,|this,on,cx|this.update(cx,|p|p.mono_menu_bar_icon=on)),
                ],
            ));
        }
        let mut roster = Vec::new();
        for (ix, mono) in self.ctx.monos.clone().into_iter().enumerate() {
            let show_id = mono.id.clone();
            let show = mono.show;
            let folders = mono.folders;
            let description = match mono.projects.split_last() {
                None => "No projects yet".to_owned(),
                Some((last, [])) => format!("Works on {last}"),
                Some((last, others)) => format!("Works on {} and {last}", others.join(", ")),
            };
            let label = div()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .debug_selector(|| "settings-mono-mascot".into())
                        .size(px(16.))
                        .flex_shrink_0()
                        .child(crate::monos::avatar::avatar(
                            &mono.mascot,
                            mono.color,
                            "idle",
                            16.,
                            true,
                        )),
                )
                .child(
                    div()
                        .debug_selector(|| "settings-mono-name".into())
                        .min_w_0()
                        .truncate()
                        .child(mono.name.clone()),
                );
            let reset = mono.clone();
            let controls = vec![
                div()
                    .text_size(px(12.))
                    .line_height(px(20.))
                    .text_color(content(0.5))
                    .child("Show Mono spawned session on the sidebar")
                    .into_any_element(),
                div()
                    .debug_selector(|| "settings-mono-sidebar-switch".into())
                    .flex_shrink_0()
                    .child(self.toggle(
                        "mono-sidebar-show",
                        format!("Show sessions started by {} in sidebar", mono.name),
                        show,
                        false,
                        cx,
                        move |_, on, cx| {
                            cx.emit(SettingsEvent::MonoSidebar(show_id.clone(), on, folders))
                        },
                    ))
                    .into_any_element(),
                icon_action(
                    "reset-mono",
                    format!("Reset {} to defaults", mono.name),
                    "rotate-ccw",
                    28.,
                    14.,
                    0.4,
                    false,
                )
                .debug_selector(|| "settings-mono-reset".into())
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_management(Target::ResetMono(reset.clone()), window, cx)
                }))
                .into_any_element(),
            ];
            roster.push(
                div()
                    .id(SharedString::from(format!("settings-mono-{}", mono.id)))
                    .child(self.row(None, ix == 0, label, Some(description.into()), controls))
                    .into_any_element(),
            );
        }
        if roster.is_empty() {
            roster.push(super::super::widgets::note("No monos yet."));
        }
        let mut groups = vec![self.group(None,true,"Monos",None,None,rows),self.group(Some("mono-list"),false,"Your monos",Some("Show or hide sessions started by each Mono. Hidden sessions remain available from the Mono's chat.".into()),None,roster),self.bots_group(cx)];
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            groups.push(self.live_group(&p, cx));
        }
        groups
    }

    /// What sits beside the menu bar icon (the tray on Windows).
    fn live_group(
        &mut self,
        p: &crate::settings::Preferences,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        use monocode_core::menu_bar as live;
        let description = if cfg!(target_os = "windows") {
            "One thing in the tray while it has something to say: a plan's usage, the agents at work, what plays, or the processor and memory. Its number shows in the icon and the rest in its tooltip. Windows may keep a new icon in the overflow until you drag it out."
        } else {
            "One thing beside the menu bar icon, as part of it, while it has something to say: a plan's usage (its logo and each window), the agents at work, what plays, or the processor and memory. The icon's menu changes it too."
        };
        let options = live::CHOICES
            .into_iter()
            .map(|choice| {
                let icon = live::usage_kind(choice)
                    .map(|kind| provider_icon(kind.id(), 14.).into_any_element());
                (
                    choice.to_owned(),
                    SharedString::from(live::name(choice)),
                    icon,
                )
            })
            .collect();
        let off = cfg!(any(target_os = "macos", target_os = "windows")) && !p.mono_menu_bar_icon;
        let select = self.select(
            "menu-bar-live",
            "Beside the icon",
            live::normalized(&p.menu_bar_live),
            options,
            Some(200.),
            cx,
            |this, choice, cx| this.update(cx, |p| p.menu_bar_live = choice),
        );
        let row = self.row(
            Some("menu-bar-live"),
            true,
            "Beside the icon",
            Some(description.into()),
            vec![
                div()
                    .when(off, |d| d.opacity(0.4))
                    .child(select)
                    .into_any_element(),
            ],
        );
        self.group(
            Some("menu-bar-live-group"),
            false,
            if cfg!(target_os = "windows") {
                "Tray"
            } else {
                "Menu bar"
            },
            None,
            None,
            vec![row],
        )
    }
}
