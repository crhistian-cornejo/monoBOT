//! What sits beside monoBOT's icon in the macOS menu bar, as part of the
//! icon, and in the Windows tray: one live item at a time (a provider's
//! usage, the agents at work, what plays, the system's load), picked from
//! the icon's menu or Settings › Monos. `monocode_core::menu_bar` decides what
//! it says, `mark` draws its picture, `mac` draws it into the icon's image and
//! the Windows tray shows it as its own icon.
//!
//! It reads what is already known: the notch's wings and player, the
//! accounts' usage (which keep refreshing on their own) and, only while the
//! system's load is chosen, one reading every two seconds. At rest it does no
//! work.
#[cfg(all(target_os = "macos", not(test)))]
pub(crate) mod mac;
pub(crate) mod mark;
#[cfg(all(target_os = "windows", not(test)))]
mod windows;

use crate::notch::Notch;
use crate::settings::Preferences;
use gpui::{App, AppContext, Context, Entity, Subscription, Task};
use monocode_core::menu_bar::{self as model, Item, Now, System, Usage};
use std::time::Duration;

#[cfg(all(target_os = "windows", not(test)))]
use windows::Bar;

/// Where the item is drawn into the menu bar icon instead (macOS), or the
/// system shows nothing of ours (tests, Linux).
#[cfg(any(test, not(target_os = "windows")))]
struct Bar;
#[cfg(any(test, not(target_os = "windows")))]
impl Bar {
    fn new(_clicks: async_channel::Sender<()>) -> Option<Self> {
        Some(Self)
    }
    fn show(&mut self, _item: Option<&Item>, _accent: [u8; 3]) {}
    fn set_visible(&mut self, _visible: bool) {}
}

/// How long after a change the item is worked out again, so a notch that
/// repaints every frame while music plays costs a few updates a second.
const SETTLE: Duration = Duration::from_millis(300);
/// How often the system is read while its load is chosen.
const SYSTEM_EVERY: Duration = Duration::from_secs(2);
/// The notch opens a moment after the workspace window: how long to look
/// for it.
const NOTCH_TRIES: u32 = 30;

pub(crate) struct LiveBar {
    choice: &'static str,
    remaining: bool,
    notch: Option<Entity<Notch>>,
    system: System,
    /// What sits beside the icon now.
    pub(crate) shown: Option<Item>,
    bar: Option<Bar>,
    settle: Option<Task<()>>,
    system_task: Option<Task<()>>,
    notch_search: Option<Task<()>>,
    subscriptions: Vec<Subscription>,
    _clicks: Task<()>,
}

impl LiveBar {
    pub(crate) fn new(prefs: &Preferences, cx: &mut Context<Self>) -> Self {
        let (sender, clicks) = async_channel::unbounded::<()>();
        let clicks = cx.spawn(async move |this, cx| {
            while clicks.recv().await.is_ok() {
                if this.update(cx, |this, cx| this.clicked(cx)).is_err() {
                    break;
                }
            }
        });
        let mut subscriptions = Vec::new();
        if let Some(accounts) = crate::accounts::get(cx) {
            subscriptions.push(cx.observe(&accounts, |this, _, cx| this.settle(cx)));
        }
        let mut this = Self {
            choice: "",
            remaining: false,
            notch: None,
            system: System::default(),
            shown: None,
            bar: Bar::new(sender),
            settle: None,
            system_task: None,
            notch_search: None,
            subscriptions,
            _clicks: clicks,
        };
        this.set_preferences(prefs, cx);
        this.look_for_notch(cx);
        this
    }

    pub(crate) fn set_preferences(&mut self, prefs: &Preferences, cx: &mut Context<Self>) {
        // The Windows tray icon follows the same switch as the macOS one.
        if let Some(bar) = &mut self.bar {
            bar.set_visible(prefs.mono_menu_bar_icon);
        }
        let choice = model::normalized(&prefs.menu_bar_live);
        if choice == self.choice && prefs.show_remaining_usage == self.remaining {
            return;
        }
        self.choice = choice;
        self.remaining = prefs.show_remaining_usage;
        self.watch_system(cx);
        self.ask_usage(cx);
        self.update_item(cx);
    }

    /// The notch's window opens after the workspace's: look for it a while.
    fn look_for_notch(&mut self, cx: &mut Context<Self>) {
        if self.find_notch(cx) {
            return;
        }
        self.notch_search = Some(cx.spawn(async move |this, cx| {
            for _ in 0..NOTCH_TRIES {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                let found = this.update(cx, |this, cx| this.find_notch(cx));
                if !matches!(found, Ok(false)) {
                    break;
                }
            }
        }));
    }

    fn find_notch(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(window) = cx
            .windows()
            .into_iter()
            .find_map(|window| window.downcast::<Notch>())
        else {
            return false;
        };
        let Some(handle) = crate::notch::Handle::from_window(window, cx) else {
            return false;
        };
        self.subscriptions
            .push(cx.observe(&handle.entity, |this, _, cx| this.settle(cx)));
        self.notch = Some(handle.entity);
        self.notch_search = None;
        self.update_item(cx);
        true
    }

    /// Reads the processor and the memory every two seconds while the
    /// system's load is chosen.
    fn watch_system(&mut self, cx: &mut Context<Self>) {
        if self.choice != "system" || cfg!(test) {
            self.system_task = None;
            self.system = System::default();
            return;
        }
        if self.system_task.is_some() {
            return;
        }
        self.system_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let stats = cx
                    .background_executor()
                    .spawn(async { monocode_core::notch::stats::read() })
                    .await;
                let system = System {
                    cpu: stats.cpu,
                    memory: stats.memory.map(|memory| memory.percent()),
                };
                let kept = this.update(cx, |this, cx| {
                    if this.system != system {
                        this.system = system;
                        this.settle(cx);
                    }
                });
                if kept.is_err() {
                    break;
                }
                cx.background_executor().timer(SYSTEM_EVERY).await;
            }
        }));
    }

    /// The accounts keep refreshing a provider's usage once it is asked for.
    fn ask_usage(&mut self, cx: &mut Context<Self>) {
        let (Some(accounts), Some(kind)) =
            (crate::accounts::get(cx), model::usage_kind(self.choice))
        else {
            return;
        };
        accounts.update(cx, |accounts, cx| {
            let key = crate::accounts::Key::new(kind, accounts.selected(kind, ""), None);
            accounts.refresh(key, false, cx);
        });
    }

    fn settle(&mut self, cx: &mut Context<Self>) {
        if self.settle.is_some() {
            return;
        }
        self.settle = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let _ = this.update(cx, |this, cx| {
                this.settle = None;
                this.update_item(cx);
            });
        }));
    }

    fn now(&self, cx: &App) -> Now {
        let notch = self.notch.as_ref().map(|notch| notch.read(cx));
        let usage = model::usage_kind(self.choice)
            .zip(crate::accounts::get(cx))
            .and_then(|(kind, accounts)| {
                let accounts = accounts.read(cx);
                let key = crate::accounts::Key::new(kind, accounts.selected(kind, ""), None);
                let snapshot = accounts.usage.get(&key)?.snapshot.as_ref()?;
                Usage::of(kind, snapshot, monocode_core::usage::now_ms())
            });
        Now {
            agents: notch.map(Notch::live_agents).unwrap_or_default(),
            music: notch.and_then(Notch::live_music),
            usage: usage.into_iter().collect(),
            system: self.system,
            remaining: self.remaining,
        }
    }

    fn update_item(&mut self, cx: &mut Context<Self>) {
        let item = model::item(self.choice, &self.now(cx));
        if item == self.shown {
            return;
        }
        if let Some(bar) = &mut self.bar {
            bar.show(item.as_ref(), accent());
        }
        self.shown = item;
        cx.notify();
    }

    /// A click on the tray icon shows monoBOT's menu, as the menu bar icon
    /// does on macOS; the Monos own it and run what is chosen.
    fn clicked(&mut self, cx: &mut Context<Self>) {
        cx.emit(TrayClicked);
    }

    /// Opens the notch where the live item lives: its agents or player,
    /// else Home. False when there is no notch.
    #[cfg(all(target_os = "windows", not(test)))]
    pub(crate) fn open_notch(&mut self, cx: &mut Context<Self>) -> bool {
        let tab = match self.choice {
            "agents" => crate::notch::Tab::Agents,
            _ => crate::notch::Tab::Home,
        };
        let Some(notch) = &self.notch else {
            return false;
        };
        notch.update(cx, |notch, cx| notch.open_on(tab, cx));
        true
    }
}

/// One line of monoBOT's icon menu, the same on macOS (`monos::status`)
/// and in the Windows tray (`windows::popup`). An action's `id` is what
/// `Monos::run_status` receives.
#[cfg(any(target_os = "macos", target_os = "windows", test))]
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Entry {
    Action {
        label: String,
        id: String,
        checked: bool,
    },
    /// A line that only informs.
    Note(String),
    Separator,
    Submenu {
        label: String,
        entries: Vec<Entry>,
    },
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
impl Entry {
    fn action(label: impl Into<String>, id: impl Into<String>) -> Self {
        Entry::Action {
            label: label.into(),
            id: id.into(),
            checked: false,
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", test))]
/// The icon's menu: each Mono opens its floating chat, then what sits
/// beside the icon, the quick composer and quitting. `open_app` adds a way
/// back to the window, which the Windows tray needs (macOS has the Dock).
pub(crate) fn status_menu(monos: &[(String, String)], beside: &str, open_app: bool) -> Vec<Entry> {
    let mut entries = Vec::new();
    if monos.is_empty() {
        entries.push(Entry::Note("No monos yet".into()));
    }
    for (id, name) in monos {
        entries.push(Entry::action(name.clone(), id.clone()));
    }
    entries.push(Entry::Separator);
    let beside = model::normalized(beside);
    let mut choices = Vec::new();
    for choice in model::CHOICES {
        choices.push(Entry::Action {
            label: model::name(choice),
            id: format!("live:{choice}"),
            checked: choice == beside,
        });
        if choice.is_empty() || choice == "usage:kimi" {
            choices.push(Entry::Separator);
        }
    }
    entries.push(Entry::Submenu {
        label: "Beside the Icon".into(),
        entries: choices,
    });
    entries.push(Entry::Separator);
    entries.push(Entry::action("Quick Composer", "composer"));
    if open_app {
        entries.push(Entry::action("Open Notch", "notch"));
        entries.push(Entry::action("Open monoBOT", "open"));
    }
    entries.push(Entry::Separator);
    entries.push(Entry::action("Quit monoBOT", "quit"));
    entries
}

/// Shows the menu at the pointer and returns the chosen action's id. It
/// runs the system's own menu loop, so call it outside any entity update.
#[cfg(all(target_os = "windows", not(test)))]
pub(crate) fn popup(entries: &[Entry]) -> Option<String> {
    windows::popup(entries)
}

/// Raised when the tray icon is clicked; the Monos show the menu.
pub(crate) struct TrayClicked;
impl gpui::EventEmitter<TrayClicked> for LiveBar {}

/// The user's accent, for a mark that needs them.
fn accent() -> [u8; 3] {
    let color: gpui::Rgba = crate::theme::accent_color().into();
    [color.r, color.g, color.b].map(|channel| (channel.clamp(0., 1.) * 255.).round() as u8)
}

/// Starts the live item for these preferences.
pub(crate) fn open(prefs: &Preferences, cx: &mut App) -> Entity<LiveBar> {
    cx.new(|cx| LiveBar::new(prefs, cx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn the_icon_menu_opens_each_mono_then_the_composer_and_quit() {
        let monos = [("m1".to_owned(), "Finance".to_owned())];
        let entries = status_menu(&monos, "agents", true);
        assert_eq!(entries[0], Entry::action("Finance", "m1"));
        let ids: Vec<&str> = entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Action { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(ids, ["m1", "composer", "notch", "open", "quit"]);
        let Some(Entry::Submenu {
            entries: choices, ..
        }) = entries
            .iter()
            .find(|entry| matches!(entry, Entry::Submenu { .. }))
        else {
            panic!("the live item's choices");
        };
        let checked: Vec<&str> = choices
            .iter()
            .filter_map(|entry| match entry {
                Entry::Action {
                    id, checked: true, ..
                } => Some(id.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, ["live:agents"]);
        let mac = status_menu(&[], "", false);
        assert_eq!(mac[0], Entry::Note("No monos yet".into()));
        assert!(!mac.contains(&Entry::action("Open monoBOT", "open")));
    }

    #[gpui::test]
    fn the_item_follows_the_choice(cx: &mut TestAppContext) {
        let mut prefs = Preferences {
            menu_bar_live: "system".into(),
            ..Preferences::default()
        };
        let bar = cx.update(|cx| open(&prefs, cx));
        bar.update(cx, |bar, cx| {
            assert!(bar.shown.is_none(), "nothing to say yet");
            bar.system = System {
                cpu: Some(42.),
                memory: Some(50.),
            };
            bar.update_item(cx);
            assert_eq!(bar.shown.as_ref().unwrap().rows, ["CPU 42%", "MEM 50%"]);
        });
        prefs.menu_bar_live = String::new();
        bar.update(cx, |bar, cx| {
            bar.set_preferences(&prefs, cx);
            assert!(bar.shown.is_none(), "nothing is chosen");
            assert_eq!(bar.system, System::default());
        });
    }
}
