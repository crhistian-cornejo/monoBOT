//! GeneralPage and UpdateRow.
use crate::notifications::updater::{self, Action};
use crate::settings::{
    SettingsView,
    widgets::{primary_button, secondary_button},
};
use crate::theme::*;
use gpui::{AnyElement, Context, FontWeight, div, prelude::*, px};

impl SettingsView {
    pub(super) fn general_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let p = self.prefs.clone();
        let alerts = vec![
            self.row(
                Some("sounds"),
                true,
                "Sounds",
                Some("Play sounds for activity, completed turns, and updates.".into()),
                vec![
                    self.toggle("sounds", "Sounds", p.sounds, false, cx, |this, on, cx| {
                        this.update(cx, |p| p.sounds = on)
                    }),
                ],
            ),
            self.row(
                Some("sounds-volume"),
                false,
                "Sound volume",
                Some("How loud the cues play, on top of your system's volume.".into()),
                vec![self.slider(
                    crate::settings::SliderKind::SoundsVolume,
                    format!("{}%", p.sounds_volume),
                    !p.sounds,
                    cx,
                )],
            ),
            self.row(
                Some("notifications"),
                false,
                "Notifications",
                Some("Alert when reminders are due or agents finish or need input.".into()),
                {
                    let mut controls = self.notification_permission_controls(cx);
                    controls.push(self.toggle(
                        "notifications",
                        "Notifications",
                        p.notifications,
                        false,
                        cx,
                        |this, on, cx| {
                            this.update(cx, |p| p.notifications = on);
                            if on {
                                this.probe_notification_permission(true, cx);
                            }
                        },
                    ));
                    controls
                },
            ),
        ];
        let mut workspace = vec![
            self.row(
                Some("file-tabs"),
                true,
                "File tabs",
                Some("Choose where files open.".into()),
                vec![self.segmented(
                    "file-tabs",
                    "File tabs",
                    &p.file_tabs,
                    &[("pane", "Beside chat"), ("workspace", "Top bar")],
                    cx,
                    |this, v, cx| this.update(cx, |p| p.file_tabs = v.into()),
                )],
            ),
            self.row(
                Some("file-word-wrap"),
                false,
                "Word wrap",
                Some("Wrap long lines to fit the editor.".into()),
                vec![self.toggle(
                    "file-word-wrap",
                    "Word wrap",
                    p.file_word_wrap,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.file_word_wrap = on),
                )],
            ),
            self.row(
                Some("tab-animations"),
                false,
                "Tab animations",
                Some("Animate tabs as they open and close.".into()),
                vec![self.toggle(
                    "tab-animations",
                    "Tab animations",
                    p.tab_animations,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.tab_animations = on),
                )],
            ),
            self.row(
                Some("notes"),
                false,
                "Notes",
                Some("Show Notes in project navigation. Mention saved notes with @note.".into()),
                vec![
                    self.toggle("notes", "Notes", p.notes, false, cx, |this, on, cx| {
                        this.update(cx, |p| p.notes = on)
                    }),
                ],
            ),
        ];
        if cfg!(any(target_os = "macos", target_os = "windows")) {
            let open = if cfg!(target_os = "macos") {
                "⌘Return"
            } else {
                "Ctrl+Return"
            };
            workspace.push(self.row(
                Some("quick-composer"),
                false,
                "Quick composer",
                Some(format!(
                    "Press {} from any app. Return starts a session in the background; {open} opens it. Change the shortcut in Keybindings.",
                    super::keybindings::quick_composer_label(&p)
                ).into()),
                {let mut controls=Vec::new();if let Some(error)=&self.ctx.quick_error{controls.push(super::super::widgets::note(error.clone()));}controls.push(self.toggle(
                    "quick-composer",
                    "Quick composer",
                    p.quick_composer,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.quick_composer = on),
                ));controls},
            ));
        }
        workspace.push(self.row(
            Some("search-panel"),
            false,
            "Search as command panel",
            Some(
                "Open search as a floating panel over your work instead of a page with its own tab."
                    .into(),
            ),
            vec![self.toggle(
                "search-panel",
                "Search as command panel",
                p.search_panel,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.search_panel = on),
            )],
        ));
        workspace.push(self.row(
            Some("working-agents"),
            false,
            "Working agents",
            Some("Show active chats across projects. Finished chats stay until opened.".into()),
            vec![self.toggle(
                "working-agents",
                "Working agents",
                p.working_agents,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.working_agents = on),
            )],
        ));
        workspace.push(self.row(
            Some("action-opens-terminal"),
            false,
            "Show terminal for actions",
            Some(
                "Open the terminal when an action runs. Off, actions run in the background.".into(),
            ),
            vec![self.toggle(
                "action-opens-terminal",
                "Show terminal for actions",
                p.action_opens_terminal,
                false,
                cx,
                |this, on, cx| this.update(cx, |p| p.action_opens_terminal = on),
            )],
        ));
        workspace.push(self.row(
            Some("terminal-placement"),
            false,
            "Terminal",
            Some("Where new terminals open: docked below the chat, as a tab in the right panel (Focused layout), or as their own tabs in the title bar.".into()),
            vec![self.segmented(
                "terminal-placement",
                "Terminal",
                p.terminal_placement(),
                &[("bottom", "Bottom"), ("panel", "Side panel"), ("title", "Title bar")],
                cx,
                |this, v, cx| this.update(cx, |p| p.set_terminal_placement(v)),
            )],
        ));
        if crate::app_window::CAN_HIDE {
            // The stored field keeps its old name, `close_to_tray`.
            let way_back = if cfg!(target_os = "macos") {
                "Closing the window hides it instead of quitting, so the notch and running agents keep going. Bring it back from the notch or the Dock, and quit with ⌘Q. Turn this off to have closing the window quit monoBOT."
            } else {
                "Closing the window hides it instead of quitting, so the notch and running agents keep going. Bring it back from the notch, the tray icon or by opening monoBOT again, and quit with Ctrl+Q or from the tray's menu. Turn this off to have closing the window quit monoBOT."
            };
            workspace.push(self.row(
                Some("close-to-tray"),
                false,
                "Keep monoBOT running in the background when the window closes",
                Some(way_back.into()),
                vec![self.toggle(
                    "close-to-tray",
                    "Keep monoBOT running in the background when the window closes",
                    p.close_to_tray,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.close_to_tray = on),
                )],
            ));
        }
        if monocode_core::keep_awake::AVAILABLE {
            workspace.push(self.row(
                Some("keep-awake"),
                false,
                "Keep computer awake",
                Some("While an agent works, or a retry, a resume or a message sent later is waiting, the computer doesn't go to sleep on its own. The screen can still turn off, and closing the lid still puts it to sleep.".into()),
                vec![self.toggle(
                    "keep-awake",
                    "Keep computer awake",
                    p.keep_awake,
                    false,
                    cx,
                    |this, on, cx| this.update(cx, |p| p.keep_awake = on),
                )],
            ));
        }
        let version = crate::notifications::updater::version();
        let about = vec![
            self.row(
                Some("update"),
                true,
                div()
                    .flex()
                    .items_baseline()
                    .gap_2()
                    .child("Version")
                    .child(
                        div()
                            .font_family(crate::fonts::mono())
                            .text_size(px(12.))
                            .font_weight(FontWeight::NORMAL)
                            .text_color(content(0.45))
                            .child(version),
                    ),
                Some(
                    cx.try_global::<updater::Shared>()
                        .and_then(|shared| updater::status(&shared.0, cx))
                        .unwrap_or_else(|| {
                            "View release notes and check for app and agent updates.".into()
                        })
                        .into(),
                ),
                vec![
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            secondary_button("whats-new", "What's new", false, false)
                                .child("What's new")
                                .on_click(cx.listener(|_, _, _, cx| {
                                    cx.emit(crate::settings::SettingsEvent::WhatsNew)
                                })),
                        )
                        .child(
                            secondary_button(
                                "check-harness-updates",
                                "Check harness updates",
                                false,
                                false,
                            )
                            .child("Check harness updates")
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(crate::settings::SettingsEvent::CheckHarnessUpdates)
                            })),
                        )
                        .child(self.app_update_button(cx))
                        .into_any_element(),
                ],
            ),
        ];
        vec![
            self.group(None, true, "Alerts", None, None, alerts),
            self.group(None, false, "Workspace", None, None, workspace),
            self.group(None, false, "About", None, None, about),
        ]
    }
}
impl SettingsView {
    /// Check for updates; once one has downloaded, restart into it; if it
    /// failed, download its installer by hand.
    fn app_update_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let updater = cx
            .try_global::<updater::Shared>()
            .map(|shared| shared.0.clone());
        match updater.as_ref().map(|u| updater::action(u, cx)) {
            Some(Action::Manual(version, url)) => {
                let label = format!("Download {version}");
                let mut ink = shell_fill();
                ink.a = 1.;
                primary_button("check-updates", label.clone(), false)
                    .child(icon("external-link", 14.).text_color(ink))
                    .child(label)
                    .on_click(move |_, _, cx| cx.open_url(&url))
                    .into_any_element()
            }
            Some(Action::Restart(version)) => {
                let label = format!("Restart to update to {version}");
                let mut ink = shell_fill();
                ink.a = 1.;
                primary_button("check-updates", label.clone(), false)
                    .child(icon("arrow-down-circle", 14.).text_color(ink))
                    .child(label)
                    .on_click(move |_, _, cx| {
                        if let Some(updater) = &updater {
                            updater::install(updater, cx)
                        }
                    })
                    .into_any_element()
            }
            Some(Action::Busy(label)) => {
                secondary_button("check-updates", label.clone(), false, true)
                    .child(icon("loader", 14.).text_color(content(0.7)))
                    .child(label)
                    .into_any_element()
            }
            Some(Action::Check) | None => {
                secondary_button("check-updates", "Check for updates", false, false)
                    .child(icon("refresh-cw", 14.).text_color(content(0.7)))
                    .child("Check for updates")
                    .on_click(cx.listener(|_, _, _, cx| {
                        cx.emit(crate::settings::SettingsEvent::CheckAppUpdates)
                    }))
                    .into_any_element()
            }
        }
    }
}
