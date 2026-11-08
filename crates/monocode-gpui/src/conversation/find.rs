//! TranscriptFind: "Find in conversation" over the chat, in project
//! sessions and in agent and group chats alike. Cmd/Ctrl+F opens it, Enter,
//! Down, F3 and Cmd/Ctrl+G step through the matches (Shift+Enter, Up and
//! the Shift variants go back), and Esc closes it. Matching is
//! `monocode_core::transcript::find`; the transcript scrolls to and marks
//! each match as a search hit does.
use super::*;
use crate::input::{self, Input};
use gpui::{App, KeyBinding, Subscription, Task, actions};
use std::time::Duration;

actions!(transcript_find, [Open, Next, Previous, Close]);

pub(crate) fn bind(cx: &mut App) {
    let m = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([
        KeyBinding::new(&format!("{m}-f"), Open, Some("Session")),
        KeyBinding::new("f3", Next, Some("Session")),
        KeyBinding::new("shift-f3", Previous, Some("Session")),
        KeyBinding::new(&format!("{m}-g"), Next, Some("Session")),
        KeyBinding::new(&format!("{m}-shift-g"), Previous, Some("Session")),
        KeyBinding::new("escape", Close, Some("TranscriptFind")),
    ]);
}

/// How long typing settles before the chat is searched.
const SETTLE: Duration = Duration::from_millis(120);

struct Bar {
    input: Entity<Input>,
    query: String,
    matches: Vec<usize>,
    active: usize,
    /// How many rows the matches were found in, to refresh them as the
    /// chat grows.
    searched: usize,
    searching: bool,
    _task: Option<Task<()>>,
    _events: Vec<Subscription>,
}

#[derive(Default)]
pub(super) struct State {
    bar: Option<Bar>,
}

impl State {
    pub(super) fn is_open(&self) -> bool {
        self.bar.is_some()
    }
}

impl Conversation {
    pub(super) fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.find.bar.is_none() {
            let input = cx.new(|cx| {
                let mut field = Input::field(
                    "Find in conversation",
                    "Find in conversation",
                    content(0.4),
                    cx,
                );
                field.set_font_size(12.);
                field
            });
            let events = vec![
                cx.subscribe(&input, |this, _, _: &input::Changed, cx| {
                    this.find_changed(cx)
                }),
                cx.subscribe(&input, |this, _, _: &input::Submitted, cx| {
                    this.step_find(1, cx)
                }),
            ];
            self.find.bar = Some(Bar {
                input,
                query: String::new(),
                matches: Vec::new(),
                active: 0,
                searched: 0,
                searching: false,
                _task: None,
                _events: events,
            });
        }
        if let Some(bar) = &self.find.bar {
            window.focus(&bar.input.focus_handle(cx), cx);
            window.dispatch_action(Box::new(input::SelectAll), cx);
        }
        cx.notify();
    }

    pub(super) fn close_find(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.find.bar.take().is_none() {
            return false;
        }
        self.transcript.update(cx, |view, cx| view.clear_search(cx));
        window.focus(&self.input.focus_handle(cx), cx);
        cx.notify();
        true
    }

    /// Drops the bar and its highlights without moving focus, as when the
    /// Agents zone or a floating panel moves to another chat.
    pub(crate) fn reset_find(&mut self, cx: &mut Context<Self>) {
        if self.find.bar.take().is_none() {
            return;
        }
        self.transcript.update(cx, |view, cx| view.clear_search(cx));
        cx.notify();
    }

    fn find_changed(&mut self, cx: &mut Context<Self>) {
        let Some(bar) = self.find.bar.as_mut() else {
            return;
        };
        let query = bar.input.read(cx).content.trim().to_owned();
        if query == bar.query {
            return;
        }
        bar.query = query.clone();
        bar.active = 0;
        bar.matches.clear();
        if query.is_empty() {
            bar.searching = false;
            bar._task = None;
            self.transcript.update(cx, |view, cx| view.clear_search(cx));
            cx.notify();
            return;
        }
        bar.searching = true;
        bar._task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SETTLE).await;
            let Ok(rows) = this.update(cx, |this, cx| {
                this.transcript.read(cx).history_rows().to_vec()
            }) else {
                return;
            };
            let searched = rows.len();
            let needle = query.clone();
            let matches = cx
                .background_executor()
                .spawn(async move { monocode_core::transcript::find::find(&rows, &needle) })
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(bar) = this.find.bar.as_mut().filter(|bar| bar.query == query) else {
                    return;
                };
                bar.matches = matches;
                bar.searched = searched;
                bar.searching = false;
                this.reveal_find(cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// Steps `delta` matches on, wrapping, after catching up with rows the
    /// chat gained since the last search.
    fn step_find(&mut self, delta: isize, cx: &mut Context<Self>) {
        let rows = self.transcript.read(cx).history_rows().len();
        let Some(bar) = self.find.bar.as_mut() else {
            return;
        };
        if bar.searching || bar.query.is_empty() {
            return;
        }
        if bar.searched != rows {
            let current = bar.matches.get(bar.active).copied();
            bar.matches = monocode_core::transcript::find::find(
                self.transcript.read(cx).history_rows(),
                &bar.query,
            );
            bar.searched = rows;
            bar.active = current
                .and_then(|row| bar.matches.iter().position(|&at| at == row))
                .unwrap_or(0);
        }
        if bar.matches.is_empty() {
            cx.notify();
            return;
        }
        let count = bar.matches.len() as isize;
        bar.active = (bar.active as isize + delta).rem_euclid(count) as usize;
        self.reveal_find(cx);
        cx.notify();
    }

    fn reveal_find(&mut self, cx: &mut Context<Self>) {
        let Some((row, query)) = self
            .find
            .bar
            .as_ref()
            .and_then(|bar| Some((*bar.matches.get(bar.active)?, bar.query.clone())))
        else {
            return;
        };
        self.transcript
            .update(cx, |view, cx| view.reveal_search(row, &query, cx));
    }

    /// The open bar's matches and the active one, for tests elsewhere.
    #[cfg(test)]
    pub(crate) fn find_matches(&self) -> Option<(Vec<usize>, usize)> {
        let bar = self.find.bar.as_ref()?;
        Some((bar.matches.clone(), bar.active))
    }

    /// The bar at the top right of the chat.
    pub(super) fn find_bar(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = self.find.bar.as_ref()?;
        let count = if bar.searching {
            "Searching…".to_owned()
        } else if bar.query.is_empty() {
            String::new()
        } else if bar.matches.is_empty() {
            "No results".to_owned()
        } else {
            format!("{} of {}", bar.active + 1, bar.matches.len())
        };
        let any = !bar.matches.is_empty();
        let button = |id: &'static str, label: &'static str, glyph: &'static str, enabled: bool| {
            div()
                .id(id)
                .debug_selector(move || id.to_owned())
                .role(Role::Button)
                .aria_label(label)
                .tooltip(chrome_tip(label, None))
                .size(px(24.))
                .flex_shrink_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .text_color(content(0.55))
                .when(enabled, |d| {
                    d.cursor_pointer()
                        .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
                })
                .when(!enabled, |d| d.opacity(0.3))
                .child(icon(glyph, 14.))
        };
        Some(
            div()
                .absolute()
                .top(px(8.))
                .left(px(12.))
                .right(px(12.))
                .flex()
                .justify_end()
                .key_context("TranscriptFind")
                .on_action(cx.listener(|this, _: &Close, window, cx| {
                    this.close_find(window, cx);
                }))
                // The field is one line: Shift+Enter and the arrows step
                // through the matches instead of moving its caret.
                .capture_action(cx.listener(|this, _: &input::Newline, _, cx| {
                    this.step_find(-1, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                    this.step_find(-1, cx);
                    cx.stop_propagation();
                }))
                .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                    this.step_find(1, cx);
                    cx.stop_propagation();
                }))
                .child(
                    div()
                        .id("transcript-find")
                        .debug_selector(|| "transcript-find".to_owned())
                        .role(Role::Search)
                        .aria_label("Find in conversation")
                        // Shrinks in a narrow pane or the floating panel.
                        .w(px(360.))
                        .max_w_full()
                        .flex()
                        .items_center()
                        .gap_1()
                        .p_1()
                        .rounded_lg()
                        .border_1()
                        .border_color(content(0.1))
                        .bg(floating_fill())
                        .shadow_lg()
                        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(icon("search", 14.).ml_1().text_color(content(0.5)))
                        .child(div().flex_1().min_w_0().child(bar.input.clone()))
                        .child(
                            div()
                                .debug_selector(|| "transcript-find-count".to_owned())
                                .flex_shrink_0()
                                .min_w(px(64.))
                                .text_right()
                                .whitespace_nowrap()
                                .font_family(crate::fonts::mono())
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(count),
                        )
                        .child(
                            button(
                                "transcript-find-previous",
                                "Previous match",
                                "chevron-up",
                                any,
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.step_find(-1, cx))),
                        )
                        .child(
                            button("transcript-find-next", "Next match", "chevron-down", any)
                                .on_click(cx.listener(|this, _, _, cx| this.step_find(1, cx))),
                        )
                        .child(
                            button("transcript-find-close", "Close find", "x", true).on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.close_find(window, cx);
                                }),
                            ),
                        ),
                )
                .into_any_element(),
        )
    }

    /// The find keys on the chat's `Session` element.
    pub(super) fn find_actions(
        &self,
        body: gpui::Stateful<gpui::Div>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        body.on_action(cx.listener(|this, _: &Open, window, cx| this.open_find(window, cx)))
            .on_action(cx.listener(|this, _: &Next, _, cx| {
                if this.find.is_open() {
                    this.step_find(1, cx);
                } else {
                    cx.propagate();
                }
            }))
            .on_action(cx.listener(|this, _: &Previous, _, cx| {
                if this.find.is_open() {
                    this.step_find(-1, cx);
                } else {
                    cx.propagate();
                }
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, WindowHandle};
    use monocode_core::store::Store;
    use monocode_core::transcript::Row;
    use std::sync::Mutex;

    fn conversation_window(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(std::path::Path::new(":memory:")).unwrap();
        let summary = store.create("claude", "/work/find").unwrap();
        cx.update(|cx| {
            input::bind(cx);
            crate::popover::bind(cx);
            bind(cx);
            cx.set_reduce_motion(true);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| Conversation::new(summary, Arc::new(Mutex::new(store)), cx))
            })
            .unwrap()
        })
    }

    fn row(role: &str, text: &str) -> Row {
        Row {
            role: role.into(),
            text: text.into(),
            first: true,
            sent_at: None,
        }
    }

    #[gpui::test]
    fn cmd_f_finds_steps_and_closes(cx: &mut TestAppContext) {
        let window = conversation_window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |c, _, cx| {
                c.summary.has_messages = true;
                c.transcript.update(cx, |view, cx| {
                    view.patch(
                        0,
                        vec![
                            row("user", "Where is the parser?"),
                            row("assistant", "The parser lives in src/parse.rs."),
                            row("user", "Thanks"),
                            row("assistant", "The PARSER is tested too."),
                        ],
                        cx,
                    )
                });
            })
            .unwrap();
        visual.run_until_parked();
        // Focus starts in the composer; Cmd/Ctrl+F opens the bar there.
        window
            .update(visual, |c, window, cx| {
                window.focus(&c.input.focus_handle(cx), cx)
            })
            .unwrap();
        let m = if cfg!(target_os = "macos") {
            "cmd"
        } else {
            "ctrl"
        };
        visual.simulate_keystrokes(&format!("{m}-f"));
        visual.run_until_parked();
        assert!(visual.debug_bounds("transcript-find").is_some());
        visual.simulate_input("parser");
        visual.executor().advance_clock(SETTLE * 2);
        visual.run_until_parked();
        let state = |visual: &mut VisualTestContext| {
            window
                .update(visual, |c, _, _| {
                    let bar = c.find.bar.as_ref().unwrap();
                    (bar.matches.clone(), bar.active)
                })
                .unwrap()
        };
        assert_eq!(state(visual), (vec![0, 1, 3], 0));
        visual.simulate_keystrokes("enter");
        assert_eq!(state(visual).1, 1);
        visual.simulate_keystrokes("shift-f3");
        visual.simulate_keystrokes("shift-f3");
        // Back past the first match wraps to the last.
        assert_eq!(state(visual).1, 2);
        let next = visual
            .debug_bounds("transcript-find-next")
            .expect("next button");
        visual.simulate_click(next.center(), Default::default());
        assert_eq!(state(visual).1, 0);
        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        assert!(visual.debug_bounds("transcript-find").is_none());
        window
            .update(visual, |c, window, cx| {
                assert!(c.input.focus_handle(cx).is_focused(window));
            })
            .unwrap();
    }

    #[gpui::test]
    fn a_query_without_matches_says_so(cx: &mut TestAppContext) {
        let window = conversation_window(cx);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |c, window, cx| {
                c.summary.has_messages = true;
                c.transcript.update(cx, |view, cx| {
                    view.patch(0, vec![row("user", "hello"), row("assistant", "hi")], cx)
                });
                c.open_find(window, cx);
            })
            .unwrap();
        visual.simulate_input("zebra");
        visual.executor().advance_clock(SETTLE * 2);
        visual.run_until_parked();
        window
            .update(visual, |c, _, _| {
                let bar = c.find.bar.as_ref().unwrap();
                assert!(bar.matches.is_empty() && !bar.searching);
            })
            .unwrap();
    }
}
