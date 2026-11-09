//! `/goal` (`monocode_core::goal`): the conversation keeps sending turns
//! toward one objective until the agent reports it done or blocked, its
//! automatic turns run out, or a turn stops or fails. The notice above the
//! composer shows where it stands, with Pause, Resume and Clear.
use super::*;
use monocode_core::goal::{self, Command, Goal, Next, Status};

impl Conversation {
    /// Handles a `/goal` message; false when the text is something else. A
    /// rejected command keeps its draft.
    pub(super) fn goal_command(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let Some(command) = goal::parse(text) else {
            return false;
        };
        if self.input.read(cx).is_composing() {
            return true;
        }
        if self.is_remote() || self.settings.mono.is_some() {
            self.status = "/goal is not available in this session.".into();
            cx.notify();
            return true;
        }
        let objective = match command {
            Command::Set(objective) => objective,
            Command::Show => {
                self.status = self.settings.goal.as_ref().map_or_else(
                    || "No goal set. Send /goal and what to work toward.".into(),
                    Goal::summary,
                );
                return self.goal_command_done(cx);
            }
            Command::Clear => {
                self.settings.goal = None;
                self.goal_due = false;
                self.status = "Goal cleared.".into();
                return self.goal_command_done(cx);
            }
            Command::Pause => {
                self.pause_goal("Paused by you.");
                return self.goal_command_done(cx);
            }
            Command::Resume => {
                self.resume_goal(cx);
                return self.goal_command_done(cx);
            }
        };
        if objective.len() > goal::MAX_OBJECTIVE {
            self.status = format!(
                "A goal is at most {} characters. Put the details in a file and mention it.",
                goal::MAX_OBJECTIVE
            );
            cx.notify();
            return true;
        }
        if self.busy
            || self.stopping
            || !self.follow_ups.state.messages.is_empty()
            || !self.history_ready()
            || self.account_switching
            || self.preparing_worktree
            || self.settings.worktree_removed
        {
            self.status =
                "Wait for the current turn and queued messages before setting a goal.".into();
            cx.notify();
            return true;
        }
        let goal = Goal::new(&objective);
        let request = self.completion.read(cx).prepare_prompt(&goal.objective);
        let attachments = std::mem::take(&mut self.attachments);
        let mut message = self.message(text.to_owned(), attachments, None, cx);
        message.prepared_prompt = Some(goal::first_prompt(&request, &goal));
        self.settings.goal = Some(goal);
        self.goal_due = false;
        self.save_settings(cx);
        self.input.update(cx, |input, cx| {
            input.take(cx);
        });
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        self.launch_message(message, cx);
        true
    }

    fn goal_command_done(&mut self, cx: &mut Context<Self>) -> bool {
        self.input.update(cx, |input, cx| {
            input.take(cx);
        });
        self.completion
            .update(cx, |completion, cx| completion.dismiss(cx));
        self.save_settings(cx);
        self.pump_goal(cx);
        cx.notify();
        true
    }

    fn pause_goal(&mut self, why: &str) {
        self.goal_due = false;
        match self.settings.goal.as_mut() {
            Some(goal) if goal.is_active() => {
                goal.status = Status::Paused;
                goal.note = Some(why.into());
                self.status = "Goal paused.".into();
            }
            Some(_) => self.status = "The goal is not running.".into(),
            None => self.status = "No goal set.".into(),
        }
    }

    /// Picks a paused or blocked goal up again with a fresh allowance. An
    /// idle conversation continues at once.
    fn resume_goal(&mut self, cx: &mut Context<Self>) {
        let Some(goal) = self.settings.goal.as_mut() else {
            self.status = "No goal set.".into();
            return;
        };
        if goal.is_active() {
            self.status = "The goal is already running.".into();
            return;
        }
        goal.resume();
        self.goal_due = !self.busy;
        self.status = "Goal resumed.".into();
        cx.notify();
    }

    /// After every turn: whether the goal goes on. `clean` is false for a
    /// turn that failed or that the user stopped.
    pub(super) fn goal_turn_ended(&mut self, clean: bool, cx: &mut Context<Self>) {
        let Some(goal) = self.settings.goal.as_mut().filter(|goal| goal.is_active()) else {
            self.goal_due = false;
            return;
        };
        let rows = self.transcript.read(cx).source_rows();
        let start = rows
            .iter()
            .rposition(monocode_core::transcript::Row::is_prompt)
            .unwrap_or(0);
        let answer: String = rows[start..]
            .iter()
            .filter(|row| row.role == "assistant")
            .map(|row| row.text.as_str())
            .collect();
        self.goal_due = goal.after_turn(&answer, clean) == Next::Continue;
    }

    /// Sends the goal's next turn once the conversation is idle. A message
    /// the user queued goes first; the goal continues after it.
    pub(super) fn pump_goal(&mut self, cx: &mut Context<Self>) {
        if !self.goal_due
            || self.busy
            || self.stopping
            || !self.follow_ups.state.messages.is_empty()
            || self.editing_last_turn.pending()
            || !self.history_ready()
            || self.account_switching
            || self.preparing_worktree
            || self.settings.worktree_removed
        {
            return;
        }
        if self.settings.usage_limit.is_some() {
            self.pause_goal("The provider's usage limit was reached.");
            self.save_settings(cx);
            return;
        }
        self.goal_due = false;
        let Some(goal) = self.settings.goal.as_mut().filter(|goal| goal.is_active()) else {
            return;
        };
        goal.begin_round();
        let label = goal.round_label();
        let prompt = goal::continuation_prompt(goal);
        self.save_settings(cx);
        let mut message = self.message(label, Vec::new(), None, cx);
        message.prepared_prompt = Some(prompt);
        self.launch_message(message, cx);
    }

    fn goal_action(&mut self, action: &str, cx: &mut Context<Self>) {
        match action {
            "pause" => self.pause_goal("Paused by you."),
            "resume" => self.resume_goal(cx),
            _ => {
                self.settings.goal = None;
                self.goal_due = false;
                self.status = "Goal cleared.".into();
            }
        }
        self.save_settings(cx);
        self.pump_goal(cx);
        cx.notify();
    }

    /// Where the goal stands, above the composer.
    pub(super) fn goal_notice(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let goal = self.settings.goal.as_ref()?;
        let toggle = match goal.status {
            Status::Active => Some(("pause", "Pause")),
            Status::Paused | Status::Blocked => Some(("resume", "Resume")),
            Status::Complete => None,
        };
        let button = |id: &'static str, label: &'static str, cx: &mut Context<Self>| {
            crate::settings::widgets::secondary_button(
                SharedString::from(format!("goal-{id}")),
                label,
                false,
                false,
            )
            .debug_selector(move || format!("goal-{id}"))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.goal_action(id, cx)))
        };
        Some(
            div()
                .debug_selector(|| "goal-notice".into())
                .mx_2()
                .mb_2()
                .p_3()
                .rounded_lg()
                .bg(content(0.04))
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .child(icon("circle-dot", 16.).text_color(accent_color()))
                .child(div().flex_1().min_w_0().child(goal.summary()))
                .children(toggle.map(|(id, label)| button(id, label, cx)))
                .child(button("clear", "Clear", cx))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, WindowHandle};
    use monocode_core::store::Store;
    use std::sync::Mutex;

    fn conversation(cx: &mut TestAppContext) -> WindowHandle<Conversation> {
        let store = Store::open(Path::new(":memory:")).unwrap();
        // Each turn fails at once, without a real provider.
        let mut summary = store.create("codex", "/missing/goal-project").unwrap();
        summary.provider = "fixture".into();
        cx.update(|cx| {
            input::bind(cx);
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view
                })
            })
            .unwrap()
        })
    }

    fn type_and_send(view: &mut Conversation, text: &str, cx: &mut Context<Conversation>) {
        view.input
            .update(cx, |input, cx| input.set_text(text.to_owned(), cx));
        view.send(cx);
    }

    /// `/goal <objective>` sets the goal and starts its first turn, which
    /// carries the rules; a turn that fails pauses it instead of continuing.
    #[gpui::test]
    fn setting_a_goal_starts_a_turn_and_a_failed_turn_pauses_it(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                type_and_send(view, "/goal make the importer pass its tests", cx);
                assert!(view.busy, "the first goal turn started");
                assert!(view.input.read(cx).content.is_empty());
                let goal = view.settings.goal.as_ref().unwrap();
                assert_eq!(goal.objective, "make the importer pass its tests");
                assert!(goal.is_active());
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, _| {
                assert!(!view.busy);
                let goal = view.settings.goal.as_ref().unwrap();
                assert_eq!(goal.status, Status::Paused, "never after a failure");
                assert_eq!(goal.rounds, 0, "no automatic turn was sent");
            })
            .unwrap();
    }

    /// A clean turn without the marker sends the next goal turn by itself,
    /// shown as a short label; the marker ends the goal.
    #[gpui::test]
    fn a_clean_turn_continues_the_goal_and_the_marker_completes_it(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.settings.goal = Some(Goal::new("port the parser"));
                view.transcript.update(cx, |transcript, cx| {
                    transcript.submit_prompt("port the parser", &view.settings, cx)
                });
                view.goal_turn_ended(true, cx);
                assert!(view.goal_due);
                view.pump_goal(cx);
                assert!(view.busy, "the next goal turn started");
                assert_eq!(view.settings.goal.as_ref().unwrap().rounds, 1);
                let rows = view.transcript.read(cx).source_rows();
                assert_eq!(
                    rows.iter().rev().find(|row| row.is_prompt()).unwrap().text,
                    "Continue the goal (1 of 8)"
                );
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |view, _, cx| {
                let goal = view.settings.goal.as_mut().unwrap();
                goal.resume();
                let mut turn = monocode_core::transcript::Transcript::default();
                turn.begin("user");
                turn.append("Continue the goal (1 of 8)");
                turn.begin("assistant");
                turn.append("Ported.\n\nGOAL: complete");
                view.transcript
                    .update(cx, |transcript, cx| transcript.patch(0, turn.rows, cx));
                view.goal_turn_ended(true, cx);
                assert!(!view.goal_due);
                assert_eq!(
                    view.settings.goal.as_ref().unwrap().status,
                    Status::Complete
                );
            })
            .unwrap();
    }

    /// A queued message goes before the goal's next turn.
    #[gpui::test]
    fn a_queued_message_goes_before_the_next_goal_turn(cx: &mut TestAppContext) {
        let window = conversation(cx);
        window
            .update(cx, |view, _, cx| {
                view.settings.goal = Some(Goal::new("x"));
                view.goal_due = true;
                let message = view.message("mine first".into(), Vec::new(), None, cx);
                view.follow_ups.state.messages.push(message);
                view.pump_goal(cx);
                assert!(!view.busy);
                assert!(view.goal_due, "it waits for the queue");
                assert_eq!(view.settings.goal.as_ref().unwrap().rounds, 0);
            })
            .unwrap();
    }

    /// The notice shows the goal with its controls; Pause, Resume and
    /// Clear act on it, and the commands do the same.
    #[gpui::test]
    fn the_notice_and_the_commands_pause_resume_and_clear(cx: &mut TestAppContext) {
        let window = conversation(cx);
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.settings.goal = Some(Goal::new("ship it"));
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("goal-notice").is_some());
        assert!(visual.debug_bounds("goal-clear").is_some());
        let pause = visual.debug_bounds("goal-pause").unwrap();
        visual.simulate_click(pause.center(), gpui::Modifiers::none());
        window
            .update(visual, |view, _, _| {
                assert_eq!(view.settings.goal.as_ref().unwrap().status, Status::Paused);
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("goal-resume").is_some());
        window
            .update(visual, |view, _, cx| {
                type_and_send(view, "/goal", cx);
                assert!(view.status.contains("Goal paused: ship it"));
                // Resuming an idle conversation sends the next turn at once.
                type_and_send(view, "/goal resume", cx);
                assert!(view.busy);
                assert_eq!(view.settings.goal.as_ref().unwrap().rounds, 1);
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |view, _, cx| {
                type_and_send(view, "/goal clear", cx);
                assert!(view.settings.goal.is_none());
                cx.notify();
            })
            .unwrap();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("goal-notice").is_none());
    }
}
