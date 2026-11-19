//! The events a turn's loop holds until its next emit, at most a `TICK`
//! (16 ms) or `BATCH_BYTES` later.
//!
//! An event that only supersedes the one before it is folded into it, so the
//! app applies, serializes and persists the latest state once per batch:
//!
//! - consecutive text, and consecutive reasoning, become one piece;
//! - a status line repeated with only text in between is dropped;
//! - a running tool update that follows another snapshot of the same call
//!   with the same metadata replaces it. Codex streams a command's output as
//!   a full copy of the call (up to 64 KB) per delta, and Claude its input per
//!   JSON fragment.
//!
//! This saves work in the app (rows rewritten, JSON serialized, contribution
//! scans), not tokens: nothing here is sent to the provider. A state change
//! (running → completed or failed), an approval, a question, an error or any
//! other event is never folded, and the order between kinds is kept.
use super::Event;
use crate::tool::ToolState;

#[derive(Default)]
pub(super) struct Batch {
    events: Vec<Event>,
    bytes: usize,
}

impl Batch {
    pub(super) fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// The size of what the batch holds now, not of everything pushed into it.
    #[cfg(test)]
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Whether to emit before the tick. A single snapshot of a running tool,
    /// like a command's 64 KB output tail replaced on every delta,
    /// waits for the tick like any other update.
    pub(super) fn full(&self, limit: usize) -> bool {
        self.bytes >= limit
            && !matches!(self.events.as_slice(), [Event::Tool(tool)] if tool.state == ToolState::Running)
    }

    pub(super) fn extend(&mut self, events: impl IntoIterator<Item = Event>) {
        for event in events {
            self.push(event);
        }
    }

    pub(super) fn push(&mut self, event: Event) {
        match event {
            Event::Text(text) => {
                if let Some(Event::Text(held)) = self.events.last_mut() {
                    held.push_str(&text);
                    self.bytes += text.len();
                    return;
                }
                self.add(Event::Text(text));
            }
            Event::Reasoning(text) => {
                if let Some(Event::Reasoning(held)) = self.events.last_mut() {
                    held.push_str(&text);
                    self.bytes += text.len();
                    return;
                }
                self.add(Event::Reasoning(text));
            }
            Event::Status(text) => {
                // The app keeps the last status of a batch; text and reasoning
                // do not change it, so the same line again says nothing new.
                let same = self
                    .events
                    .iter()
                    .rev()
                    .find(|e| !matches!(e, Event::Text(_) | Event::Reasoning(_)))
                    .is_some_and(|e| matches!(e, Event::Status(held) if *held == text));
                if !same {
                    self.add(Event::Status(text));
                }
            }
            Event::Tool(tool) => match self.events.last_mut() {
                Some(Event::Tool(held))
                    if held.id == tool.id
                        && held.state == ToolState::Running
                        && tool.state == ToolState::Running
                        && held.name == tool.name
                        && held.panel == tool.panel
                        && held.display == tool.display =>
                {
                    self.bytes -= tool_bytes(held);
                    self.bytes += tool_bytes(&tool);
                    *held = tool;
                }
                _ => self.add(Event::Tool(tool)),
            },
            event => self.add(event),
        }
    }

    pub(super) fn take(&mut self) -> Vec<Event> {
        self.bytes = 0;
        std::mem::take(&mut self.events)
    }

    fn add(&mut self, event: Event) {
        self.bytes += size(&event);
        self.events.push(event);
    }
}

fn tool_bytes(tool: &crate::tool::ToolCall) -> usize {
    tool.input.len() + tool.output.len()
}

fn size(event: &Event) -> usize {
    match event {
        Event::Text(text) | Event::Reasoning(text) | Event::Status(text) => text.len(),
        Event::Tool(tool) => tool_bytes(tool),
        Event::Approval(approval) => approval.detail.len(),
        Event::Question(question) => question.items.iter().map(|q| q.prompt.len()).sum(),
        _ => 64,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::ToolCall;

    fn tool(id: &str, state: ToolState, output: &str) -> Event {
        let mut call = ToolCall::new(id, "commandExecution");
        call.state = state;
        call.output = output.into();
        Event::Tool(call)
    }

    #[test]
    fn text_and_reasoning_fold_without_changing_order_between_kinds() {
        let mut batch = Batch::default();
        batch.extend([
            Event::BeginAssistant,
            Event::Reasoning("a".into()),
            Event::Status("Thinking…".into()),
            Event::Reasoning("b".into()),
            Event::Status("Thinking…".into()),
            Event::Text("Hel".into()),
            Event::Text("lo".into()),
            Event::Status("Working…".into()),
        ]);
        assert_eq!(
            batch.take(),
            vec![
                Event::BeginAssistant,
                Event::Reasoning("a".into()),
                Event::Status("Thinking…".into()),
                Event::Reasoning("b".into()),
                Event::Text("Hello".into()),
                Event::Status("Working…".into()),
            ]
        );
        assert_eq!(batch.bytes(), 0);
    }

    #[test]
    fn a_status_after_anything_that_sets_one_is_kept() {
        let mut batch = Batch::default();
        batch.extend([
            Event::Status("Thinking…".into()),
            tool("t", ToolState::Running, ""),
            Event::Status("Thinking…".into()),
        ]);
        assert_eq!(batch.take().len(), 3);
    }

    #[test]
    fn tool_updates_fold_only_within_one_state_of_one_call() {
        let mut batch = Batch::default();
        batch.extend([
            tool("a", ToolState::Running, "1"),
            tool("a", ToolState::Running, "12"),
            tool("b", ToolState::Running, "x"),
            tool("a", ToolState::Running, "123"),
            tool("a", ToolState::Failed, "123\nerror: boom"),
            tool("a", ToolState::Failed, "final"),
        ]);
        assert_eq!(batch.bytes(), 2 + 1 + 3 + 15 + 5);
        assert_eq!(
            batch.take(),
            vec![
                tool("a", ToolState::Running, "12"),
                tool("b", ToolState::Running, "x"),
                tool("a", ToolState::Running, "123"),
                tool("a", ToolState::Failed, "123\nerror: boom"),
                tool("a", ToolState::Failed, "final"),
            ]
        );
    }

    #[test]
    fn large_text_flushes_before_the_tick_but_a_running_tool_can_wait() {
        let mut batch = Batch::default();
        batch.extend([Event::Text("abc".into()), Event::Text("de".into())]);
        assert!(batch.full(5));
        batch.take();
        batch.push(tool("a", ToolState::Running, "abcde"));
        assert!(!batch.full(5));
        batch.push(Event::Done);
        assert!(batch.full(5));
    }

    /// Measures what the app applies for a Codex command that streams 2,000
    /// output lines after 200 reasoning deltas, real frames through the
    /// parser, 32 frames per tick (the parser's output, not a live CLI's
    /// timing). App work only: no byte of this goes to the provider.
    #[test]
    fn codex_stream_measured_through_the_parser() {
        use serde_json::json;
        let mut p = crate::provider::tests::protocol(crate::provider::Kind::Codex);
        let mut out = Vec::new();
        // Codex reasons, then runs the command and streams its output.
        let mut frames: Vec<_> = (0..200)
            .map(|_| {
                json!({"method":"item/reasoning/textDelta",
                    "params":{"itemId":"r","delta":"checking the suite. "}})
            })
            .collect();
        frames.push(json!({"method":"item/started","params":{"item":{
            "type":"commandExecution","id":"cmd","command":"cargo test","status":"inProgress"}}}));
        for i in 0..2000 {
            frames.push(json!({"method":"item/commandExecution/outputDelta",
                "params":{"itemId":"cmd","delta":format!("test case {i:04} ... ok, still running the suite\n")}}));
        }
        let aggregated = format!(
            "{}error[E0308]: mismatched types\n",
            "test case ... ok, still running the suite\n".repeat(1999)
        );
        frames.push(json!({"method":"item/completed","params":{"item":{
            "type":"commandExecution","id":"cmd","command":"cargo test",
            "status":"failed","aggregatedOutput":aggregated}}}));
        let mut raw = Vec::new();
        let mut batched = Vec::new();
        let mut batch = Batch::default();
        for (n, frame) in frames.into_iter().enumerate() {
            let events = p.frame(frame, &mut out).unwrap();
            raw.extend(events.iter().cloned());
            batch.extend(events);
            if n % 32 == 31 {
                batched.extend(batch.take());
            }
        }
        batched.extend(batch.take());
        // What the app does per event: a tool event rewrites its row as JSON.
        let work = |events: &[Event]| {
            let tool_json: usize = events
                .iter()
                .filter_map(|e| match e {
                    Event::Tool(t) => Some(serde_json::to_string(t).unwrap().len()),
                    _ => None,
                })
                .sum();
            (events.len(), tool_json)
        };
        let (raw_events, raw_json) = work(&raw);
        let (events, json) = work(&batched);
        eprintln!(
            "codex stream: {raw_events} events / {raw_json} tool JSON bytes before, \
             {events} events / {json} tool JSON bytes after"
        );
        // Measured: 2,402 → 85 events, 87,532,040 → 2,847,716 bytes of JSON.
        assert_eq!((raw_events, events), (2402, 85));
        assert!(json * 25 < raw_json, "{json} vs {raw_json}");
        // Same end state: the full reasoning, the failure and its error line.
        let reasoning = |events: &[Event]| -> String {
            events
                .iter()
                .filter_map(|e| match e {
                    Event::Reasoning(t) => Some(t.as_str()),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(reasoning(&raw), reasoning(&batched));
        let last_tool = |events: &[Event]| {
            events
                .iter()
                .rev()
                .find_map(|e| match e {
                    Event::Tool(t) => Some(t.clone()),
                    _ => None,
                })
                .unwrap()
        };
        let end = last_tool(&batched);
        assert_eq!(end, last_tool(&raw));
        assert_eq!(end.state, ToolState::Failed);
        assert!(end.output.ends_with("error[E0308]: mismatched types\n"));
    }

    #[test]
    fn approvals_and_errors_are_never_folded() {
        use crate::agent::{Approval, Question, QuestionItem};
        let approval = Event::Approval(Approval {
            id: "a".into(),
            title: "Run tests".into(),
            detail: "cargo test".into(),
            can_remember: false,
        });
        let question = Event::Question(Question {
            id: "q".into(),
            title: "Confirm".into(),
            items: vec![QuestionItem {
                id: "q1".into(),
                prompt: "Which test?".into(),
                multiple: false,
                allow_custom: true,
                options: Vec::new(),
            }],
            auto_resolve_at: None,
        });
        let error = Event::FollowUpFailed {
            id: "1".into(),
            error: "e".into(),
        };
        let events = [
            Event::SessionId("thread".into()),
            approval.clone(),
            approval,
            question.clone(),
            question,
            error.clone(),
            error,
            Event::Done,
        ];
        let mut batch = Batch::default();
        batch.extend(events.clone());
        assert_eq!(batch.take(), events);
    }
}
