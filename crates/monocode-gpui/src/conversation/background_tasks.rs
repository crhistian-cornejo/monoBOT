//! Commands and subagents the agent left running after its reply. They show
//! in the chat, under the reply (`transcript/background.rs`); the
//! conversation keeps them too, so a message sent meanwhile steers instead of
//! waiting behind a server that never ends.
use super::*;
use monocode_core::provider::BackgroundTask;

#[derive(Default)]
pub(super) struct BackgroundTasks {
    pub tasks: Vec<BackgroundTask>,
}

impl Conversation {
    pub(super) fn set_background(&mut self, tasks: Vec<BackgroundTask>, cx: &mut Context<Self>) {
        if self.background.tasks == tasks {
            return;
        }
        self.background.tasks = tasks.clone();
        self.transcript
            .update(cx, |view, cx| view.set_background(tasks, cx));
        cx.notify();
    }

    pub(super) fn clear_background(&mut self, cx: &mut Context<Self>) {
        self.set_background(Vec::new(), cx);
    }

    /// Whether a message sent now reaches the agent mid-turn. Antigravity
    /// takes one only while it waits on the commands it left running.
    pub(super) fn provider_steers(&self) -> bool {
        let kind = Kind::from_id(&self.summary.provider);
        kind.can_steer() || (!self.background.tasks.is_empty() && kind.steers_while_waiting())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::store::Store;
    use std::sync::Mutex;

    fn task(id: &str, agent: bool) -> BackgroundTask {
        BackgroundTask {
            id: id.into(),
            description: format!("Task {id}"),
            agent,
            command: (!agent).then(|| "npm run dev".into()),
            started_at: monocode_core::time::now_ms(),
            output: Vec::new(),
        }
    }

    #[gpui::test]
    fn background_work_steers_new_messages_and_clears_on_stop(cx: &mut gpui::TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("claude", "/tmp").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::settings::Preferences {
                follow_up: "queue".into(),
                ..Default::default()
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view.busy = true;
                    view.set_background(vec![task("dev", false), task("review", true)], cx);
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        window
            .update(visual, |view, _, cx| {
                assert_eq!(view.transcript.read(cx).background_count(), 2);
                view.enqueue_follow_up("Is the server up?".into(), None, cx);
                assert!(
                    view.follow_ups.state.messages[0].steer,
                    "a message must not wait behind a server that never ends"
                );
                view.stop(cx);
                assert!(view.background.tasks.is_empty());
                assert_eq!(view.transcript.read(cx).background_count(), 0);
            })
            .unwrap();
    }

    #[gpui::test]
    fn antigravity_takes_a_message_only_while_its_commands_run(cx: &mut gpui::TestAppContext) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store.create("antigravity", "/tmp").unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            cx.set_reduce_motion(true);
            cx.set_global(crate::settings::Preferences {
                follow_up: "steer".into(),
                ..Default::default()
            });
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                    view.loaded = true;
                    view.summary.has_messages = true;
                    view.busy = true;
                    view
                })
            })
            .unwrap()
        });
        let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |view, _, cx| {
                view.enqueue_follow_up("While it thinks".into(), None, cx);
                assert!(
                    !view.follow_ups.state.messages[0].steer,
                    "mid-answer, Antigravity's messages wait their turn"
                );
                view.set_background(vec![task("clippy", false)], cx);
                assert!(view.provider_steers());
                view.enqueue_follow_up("Is clippy done?".into(), None, cx);
                assert!(
                    view.follow_ups.state.messages[1].steer,
                    "once it waits on a command, a message reaches it now"
                );
            })
            .unwrap();
    }
}
