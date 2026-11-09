//! The shell's side of the notch: it opens the notch's window once the
//! workspace window is up, feeds it what the sessions and the group call are
//! doing, and carries out what is asked from it (open a chat, answer an
//! approval, start a quick chat, hang up).
use super::*;
use crate::notch::{Handle, NotchEvent};
use crate::notifications::approvals::InputKind;
use monocode_core::notch::{self as model, Activity, Ask, Call, Face, Feed, Recent, Upcoming};
use std::path::Path;

impl Shell {
    /// Reads the saved look off the interface thread, then opens the notch.
    pub(super) fn start_notch(&mut self, cx: &mut Context<Self>) {
        // Tests drive the notch on its own; the shell's tests open no notch.
        if self.notch.is_some()
            || cfg!(test)
            || !cfg!(any(target_os = "macos", target_os = "windows"))
        {
            return;
        }
        crate::notch::follow_reduced_motion(cx);
        let store = self.store.clone();
        let saved = cx.background_executor().spawn(async move {
            let store = locked(&store).ok();
            let look = store
                .as_ref()
                .and_then(|store| store.setting(model::SETTING).ok().flatten());
            let data = store.and_then(|store| store.data_dir().map(Path::to_path_buf));
            (look, data)
        });
        cx.spawn(async move |this, cx| {
            let (saved, data) = saved.await;
            let _ = this.update(cx, |this, cx| {
                this.open_notch(model::read(saved.as_deref()), cx);
                if let Some(data) = data {
                    this.start_notch_hooks(&data, cx);
                }
            });
        })
        .detach();
    }

    /// Claude Code's and Codex's own sessions: a socket the hooks write to,
    /// and the hooks added to their settings (once; see `notch::hooks`).
    fn start_notch_hooks(&mut self, data: &Path, cx: &mut Context<Self>) {
        #[cfg(unix)]
        {
            use monocode_core::notch::hooks;
            let mut socket = data.join("notch.sock");
            // A Unix socket's path holds at most 104 bytes.
            if socket.as_os_str().len() > 100 {
                socket = std::env::temp_dir().join(format!(
                    "monobot-notch-{}.sock",
                    hooks::external_id(&data.to_string_lossy()).unsigned_abs()
                ));
            }
            let (sender, events) = async_channel::unbounded();
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            cx.background_executor()
                .spawn(async move {
                    if let Err(error) = hooks::serve(&socket, move |event| {
                        let _ = sender.try_send(event);
                    }) {
                        eprintln!("notch hooks: could not listen on {socket:?}: {error}");
                        return;
                    }
                    if let Some(home) = home {
                        for (path, result) in hooks::install(&home, &socket) {
                            match result {
                                Ok(true) => eprintln!("notch hooks: added to {path:?}"),
                                Ok(false) => {}
                                Err(error) => eprintln!("notch hooks: {path:?}: {error}"),
                            }
                        }
                    }
                })
                .detach();
            self.notch_hooks = Some(cx.spawn(async move |this, cx| {
                while let Ok(event) = events.recv().await {
                    let alive = this.update(cx, |this, cx| {
                        this.notch_externals
                            .apply(event, monocode_core::time::now_ms());
                        this.sync_notch(cx);
                    });
                    if alive.is_err() {
                        break;
                    }
                }
            }));
        }
        #[cfg(not(unix))]
        let _ = (data, cx);
    }

    fn open_notch(&mut self, look: model::Look, cx: &mut Context<Self>) {
        let store = self.store.clone();
        let save: crate::notch::Save = std::rc::Rc::new(move |json: String, cx: &mut App| {
            let store = store.clone();
            cx.background_executor()
                .spawn(async move {
                    if let Ok(store) = locked(&store) {
                        let _ = store.save_setting(model::SETTING, &json);
                    }
                })
                .detach();
        });
        let Some(window) = crate::notch::open(look, save, cx) else {
            return;
        };
        let Some(handle) = Handle::from_window(window, cx) else {
            return;
        };
        self.observations.push(
            cx.subscribe(&handle.entity, |this, _, event: &NotchEvent, cx| {
                this.notch_event(event.clone(), cx)
            }),
        );
        // MONOBOT_NOTCH_TRACE=1: opens the panel and walks its pages, so
        // their timing can be read without touching the pointer.
        if crate::notch::touring() {
            let entity = handle.entity.clone();
            cx.spawn(async move |_, cx| {
                let tabs = crate::notch::Tab::ALL;
                for step in 0..12usize {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(1500))
                        .await;
                    entity.update(cx, |notch, cx| {
                        eprintln!(
                            "NOTCHTRACE {} step {step}",
                            monocode_core::time::now_ms() % 100_000
                        );
                        notch.set_open(true, cx);
                        notch.set_tab(tabs[step % tabs.len()], cx);
                    });
                }
            })
            .detach();
        }
        self.notch = Some(handle);
        self.sync_notch_now(cx);
    }

    /// Asks for the notch to catch up. Sessions change on every streamed
    /// token, so the work is gathered at most four times a second.
    pub(super) fn sync_notch(&mut self, cx: &mut Context<Self>) {
        if self.notch.is_none() || self.notch_sync.is_some() {
            return;
        }
        self.notch_sync = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(250))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.notch_sync = None;
                this.sync_notch_now(cx);
            });
        }));
    }

    /// What the notch shows: the live agents card's sessions, every pending
    /// approval or question, and the group call.
    fn sync_notch_now(&mut self, cx: &mut Context<Self>) {
        let Some(notch) = self.notch.clone() else {
            return;
        };
        let mut activities: Vec<Activity> = self
            .live_agents(cx)
            .into_iter()
            .map(|agent| Activity {
                steps: self
                    .sessions
                    .get(&agent.id)
                    .and_then(|view| view.read(cx).transcript.read(cx).live_steps()),
                id: agent.id,
                provider: agent.provider,
                title: agent.title,
                step: agent.activity,
                started_at: agent.started_at,
                needs_you: agent.needs_approval,
                done: agent.done,
                agent: None,
                face: None,
            })
            .collect();
        // The agents' own chats (Monos, their habits) work outside the
        // sidebar; the notch shows them too, by the agent's name.
        if let Some(monos) = &self.monos {
            let monos = monos.read(cx);
            let seats = monos.notch_seats(cx);
            let chats = monos
                .all_notification_chats(cx)
                .into_iter()
                .map(|view| {
                    let mono = monos.notification_mono(view.read(cx).summary.id);
                    let name = mono.map(|mono| mono.label());
                    let face = mono.map(|mono| (mono.mascot.clone(), mono.color));
                    (view, name, face)
                })
                .chain(
                    seats
                        .into_iter()
                        .map(|(view, name, face)| (view, Some(name), face)),
                );
            for (view, name, face) in chats {
                let chat = view.read(cx);
                let id = chat.summary.id;
                if !chat.responding() || activities.iter().any(|a| a.id == id) {
                    continue;
                }
                let transcript = chat.transcript.read(cx);
                let (tool, started_at, _) = transcript.live_turn(&chat.summary.cwd);
                let needs_you = chat.needs_approval();
                activities.push(Activity {
                    id,
                    provider: chat.summary.provider.clone(),
                    title: chat.display_title().to_string(),
                    step: if needs_you {
                        "Need approval".into()
                    } else {
                        tool.unwrap_or_else(|| "Working".into())
                    },
                    started_at,
                    needs_you,
                    done: false,
                    agent: name,
                    face: face.map(|(mascot, color)| Face { mascot, color }),
                    steps: transcript.live_steps(),
                });
            }
        }
        // Claude Code and Codex in a terminal, through their hooks.
        self.notch_externals.prune(monocode_core::time::now_ms());
        activities.extend(self.notch_externals.activities());
        let mut asks = Vec::new();
        let seats = self.notch_seat_chats(cx);
        for view in self.notification_chats(cx).into_iter().chain(seats) {
            let chat = view.read(cx);
            if asks.iter().any(|ask: &Ask| ask.session == chat.summary.id) {
                continue;
            }
            for input in chat.pending_notification_inputs(cx) {
                asks.push(Ask {
                    session: chat.summary.id,
                    request: input.request_id,
                    provider: chat.summary.provider.clone(),
                    title: chat.display_title().to_string(),
                    label: input.label,
                    approval: input.kind == InputKind::Approval,
                });
            }
        }
        let call = self
            .monos
            .as_ref()
            .and_then(|monos| monos.read(cx).notch_call())
            .map(|(group, status)| Call { group, status });
        let mut recent: Vec<Recent> = self
            .session_summaries
            .values()
            .filter(|session| !session.archived && session.has_messages)
            .map(|session| Recent {
                id: session.id,
                provider: session.provider.clone(),
                title: if session.title.trim().is_empty() {
                    "Untitled chat".into()
                } else {
                    session.title.clone()
                },
                updated_at: session.updated_at,
            })
            .collect();
        recent.sort_by_key(|recent| std::cmp::Reverse(recent.updated_at));
        recent.truncate(6);
        let now = monocode_core::time::now_ms();
        let mut upcoming: Vec<Upcoming> = self
            .reminders
            .iter()
            .filter(|reminder| reminder.fired_at.is_none() && reminder.due_at >= now)
            .map(|reminder| Upcoming {
                session: reminder.session_id,
                title: self
                    .session_summaries
                    .get(&reminder.session_id)
                    .map_or_else(|| "Reminder".into(), |session| session.title.clone()),
                due_at: reminder.due_at,
            })
            .collect();
        upcoming.sort_by_key(|reminder| reminder.due_at);
        let feed = Feed {
            activities,
            asks,
            call,
            recent,
            upcoming,
        };
        notch
            .entity
            .update(cx, |notch, cx| notch.update_feed(feed, cx));
    }

    /// The chats of group members in their seats.
    fn notch_seat_chats(&self, cx: &App) -> Vec<Entity<Conversation>> {
        self.monos.as_ref().map_or_else(Vec::new, |monos| {
            monos
                .read(cx)
                .notch_seats(cx)
                .into_iter()
                .map(|(view, _, _)| view)
                .collect()
        })
    }

    fn notch_event(&mut self, event: NotchEvent, cx: &mut Context<Self>) {
        use crate::notifications::delivery::Target;
        match event {
            // A terminal's session has no chat here to open.
            NotchEvent::OpenSession(id) if id < 0 => {}
            NotchEvent::OpenSession(id) => {
                // The notch is another app's neighbor: bring MonoBOT forward
                // with the chat, as a notification's click does.
                cx.activate(true);
                self.open_notice_target(Target::Session(id), cx)
            }
            NotchEvent::Decide {
                session,
                request,
                decision,
            } => {
                let seat = || {
                    self.notch_seat_chats(cx)
                        .into_iter()
                        .find(|view| view.read(cx).summary.id == session)
                };
                if let Some(view) = self.notification_chat(session, cx).or_else(seat) {
                    view.update(cx, |view, cx| {
                        view.decide_notification_request(&request, decision, cx)
                    });
                }
                self.sync_notch_now(cx);
            }
            NotchEvent::NewChat => {
                cx.activate(true);
                cx.spawn(async move |this, cx| {
                    let _ =
                        this.update_in(cx, |this, window, cx| this.show_quick_composer(window, cx));
                })
                .detach();
            }
            NotchEvent::Ask(prompt) => {
                cx.activate(true);
                cx.spawn(async move |this, cx| {
                    let _ = this.update_in(cx, |this, window, cx| {
                        let mut launch = this.quick_choice(cx);
                        launch.prompt = prompt;
                        this.launch_quick_composer(launch, window, cx);
                    });
                })
                .detach();
            }
            NotchEvent::HangUp => {
                if let Some(monos) = self.monos.clone() {
                    monos.update(cx, |monos, cx| monos.call_hang_up(cx));
                }
                self.sync_notch(cx);
            }
        }
    }
}
