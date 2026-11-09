//! A group call through a live model (Settings › Voice › Calls talk
//! through): Gemini Live or Phonic hosts the call with one voice, and each
//! request for a member goes to that member's own seat through `ask_member`,
//! whose answer the host then tells the user. Audio and the WebSocket run on
//! their own threads (`monobot_live`, `monobot_voice::duplex`); this module
//! only connects them to the call's lines and seats.
use super::*;
use monobot_live::session::{Host, Options, Session, State};
use monobot_live::transport::Network;
use monobot_live::{Event, ToolCall, gemini, phonic};
use monobot_voice::duplex::Duplex;
use monocode_core::mono::groups::live as plan;
use monocode_core::voice::{Provider, keys};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

/// How long a member may take before the host hears it gave up.
const MEMBER_PATIENCE: Duration = Duration::from_secs(10 * 60);

pub(in crate::monos) struct LiveCall {
    pub(in crate::monos) provider: Provider,
    session: Option<Session>,
    _duplex: Option<Arc<Duplex>>,
    asks: Vec<Ask>,
    /// The lines being written now: the user's words and the host's.
    heard: Option<usize>,
    said: Option<usize>,
    _messages: Option<Task<()>>,
}

struct Ask {
    request: String,
    member: String,
    name: String,
    line: usize,
    reply: std::sync::mpsc::Sender<Result<String, String>>,
}

pub(in crate::monos) enum Message {
    State(State),
    Heard(String),
    Said(String),
    TurnDone,
    Interrupted,
    Ask {
        member: String,
        request: String,
        reply: std::sync::mpsc::Sender<Result<String, String>>,
    },
}

/// The session's side of the app: audio straight to the speakers, the rest
/// to the interface thread.
pub(in crate::monos) struct LiveHost {
    messages: async_channel::Sender<Message>,
    duplex: Mutex<Option<Arc<Duplex>>>,
    rate: u32,
    members: Vec<Member>,
}

impl Host for LiveHost {
    fn event(&self, event: &Event) {
        let duplex = self
            .duplex
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        let message = match event {
            Event::Audio(samples) => {
                if let Some(duplex) = duplex {
                    duplex.play(samples.clone(), self.rate);
                }
                return;
            }
            Event::Interrupted => {
                if let Some(duplex) = duplex {
                    duplex.flush();
                }
                Message::Interrupted
            }
            Event::Heard(text) => Message::Heard(text.clone()),
            Event::Said(text) => Message::Said(text.clone()),
            Event::TurnDone => Message::TurnDone,
            _ => return,
        };
        let _ = self.messages.try_send(message);
    }

    fn state(&self, state: State) {
        let _ = self.messages.try_send(Message::State(state));
    }

    fn tool(&self, call: &ToolCall, cancelled: &AtomicBool) -> serde_json::Value {
        if call.name != plan::ASK_MEMBER {
            return serde_json::json!({"error": format!("There is no tool called {}.", call.name)});
        }
        let (member, request) = match plan::ask_target(&self.members, &call.args) {
            Ok(target) => target,
            Err(why) => return serde_json::json!({"error": why}),
        };
        let name = self
            .members
            .iter()
            .find(|candidate| candidate.id == member)
            .map_or_else(|| member.clone(), |member| member.name.clone());
        let (reply, answered) = std::sync::mpsc::channel();
        if self
            .messages
            .try_send(Message::Ask {
                member,
                request,
                reply,
            })
            .is_err()
        {
            return plan::answer(&name, Err("The call ended."));
        }
        let until = Instant::now() + MEMBER_PATIENCE;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return plan::answer(&name, Err("Cancelled."));
            }
            match answered.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(text)) => return plan::answer(&name, Ok(&text)),
                Ok(Err(why)) => return plan::answer(&name, Err(&why)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) if Instant::now() < until => {}
                Err(_) => return plan::answer(&name, Err("It did not answer in time.")),
            }
        }
    }
}

/// What a member does, for the host's instructions.
fn about(member: &Member) -> String {
    use monocode_core::mono::groups::brief::MemberKind;
    match &member.kind {
        MemberKind::Bot { role } => role.clone(),
        MemberKind::Workstream => "runs a workstream".into(),
        MemberKind::Coding { projects } if projects.is_empty() => "coding agent".into(),
        MemberKind::Coding { projects } => format!("coding agent on {}", projects.join(", ")),
    }
}

impl Monos {
    /// The live provider Settings chose, if any.
    pub(in crate::monos) fn call_live_choice(&self) -> Option<Provider> {
        Provider::from_id(&self.prefs.voice_live)
    }

    /// Starts the call through `provider`. Reading the key, opening the
    /// microphone and connecting all happen off the interface thread; without
    /// a key the call falls back to this Mac's voices.
    pub(in crate::monos) fn call_start_live(&mut self, provider: Provider, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Some(group) = self.roster.get(&call.group).cloned() else {
            return;
        };
        call.epoch = next_generation();
        call.phase = Phase::Starting;
        call.started_at = now_ms();
        let (group_id, epoch, members) = (call.group.clone(), call.epoch, call.members.clone());
        let goal = group.group.as_ref().and_then(|group| group.goal.clone());
        let instructions = plan::instructions(
            &group.label(),
            goal.as_deref(),
            &members
                .iter()
                .map(|member| (member.name.clone(), about(member)))
                .collect::<Vec<_>>(),
        );
        let names: Vec<String> = members.iter().map(|member| member.name.clone()).collect();
        let tools = vec![gemini::Tool {
            name: plan::ASK_MEMBER.into(),
            description: plan::ask_member_description().into(),
            parameters: plan::ask_member_schema(&names),
        }];
        let (messages, incoming) = async_channel::unbounded();
        let opened = cx.background_executor().spawn(async move {
            let Some(key) = keys::get(provider) else {
                return Err(None);
            };
            let rate = match provider {
                Provider::Gemini => gemini::OUTPUT_RATE,
                Provider::Phonic => phonic::OUTPUT_RATE,
            };
            let host = Arc::new(LiveHost {
                messages,
                duplex: Mutex::new(None),
                rate,
                members,
            });
            let session = match provider {
                Provider::Gemini => Session::start(
                    gemini::Gemini::new(key.expose(), &instructions, tools),
                    Network,
                    host.clone(),
                    Options::default(),
                ),
                Provider::Phonic => Session::start(
                    phonic::Phonic::new(key.expose(), &instructions, tools),
                    Network,
                    host.clone(),
                    Options {
                        remind_every: Some(Duration::from_secs(20)),
                        ..Options::default()
                    },
                ),
            };
            let session = Arc::new(Mutex::new(Some(session)));
            let mic_session = session.clone();
            let duplex = Duplex::start(move |frame| {
                if let Some(session) = mic_session
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .as_ref()
                {
                    session.send_audio(frame);
                }
            })
            .map_err(Some)?;
            let duplex = Arc::new(duplex);
            *host.duplex.lock().unwrap_or_else(|e| e.into_inner()) = Some(duplex.clone());
            let session = session
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .ok_or(None)?;
            Ok((session, duplex))
        });
        cx.spawn(async move |this, cx| {
            let opened = opened.await;
            let _ = this.update(cx, |this, cx| {
                this.call_live_opened(&group_id, epoch, provider, opened, incoming, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn call_live_opened(
        &mut self,
        group: &str,
        epoch: u64,
        provider: Provider,
        opened: Result<(Session, Arc<Duplex>), Option<String>>,
        incoming: async_channel::Receiver<Message>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.group == group && call.epoch == epoch)
        else {
            // Hung up meanwhile: the session ends as it drops.
            return;
        };
        let (session, duplex) = match opened {
            Ok(opened) => opened,
            Err(None) => {
                call.phase = Phase::Idle;
                call.error = Some(format!(
                    "No {} key is saved, so this call talks on this Mac. Add one in Settings › Voice.",
                    provider.label()
                ));
                self.call_start_local(cx);
                return;
            }
            Err(Some(why)) => {
                call.phase = Phase::Idle;
                call.error = Some(why);
                cx.notify();
                return;
            }
        };
        let group = group.to_owned();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(message) = incoming.recv().await {
                let alive = this.update(cx, |this, cx| {
                    this.call_live_message(&group, epoch, message, cx)
                });
                if alive.is_err() {
                    return;
                }
            }
        });
        call.live = Some(LiveCall {
            provider,
            session: Some(session),
            _duplex: Some(duplex),
            asks: Vec::new(),
            heard: None,
            said: None,
            _messages: Some(task),
        });
        cx.notify();
    }

    pub(in crate::monos) fn call_live_message(
        &mut self,
        group: &str,
        epoch: u64,
        message: Message,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.group == group && call.epoch == epoch)
        else {
            return;
        };
        let Some(live) = call.live.as_mut() else {
            return;
        };
        let host_name = live.provider.label().to_owned();
        match message {
            Message::State(State::Live) => {
                call.phase = Phase::Listening;
                call.error = None;
            }
            Message::State(State::Connecting | State::Reconnecting) => {}
            Message::State(State::Ended(why)) => {
                call.error = why;
                self.call_hang_up(cx);
                return;
            }
            Message::Heard(text) => {
                live.said = None;
                let line = *live.heard.get_or_insert_with(|| {
                    call.lines.push(Line {
                        value: CallLine {
                            who: "user".into(),
                            name: "You".into(),
                            text: String::new(),
                            status: LineStatus::Said,
                        },
                        note: None,
                    });
                    call.lines.len() - 1
                });
                call.lines[line].value.text.push_str(&text);
                call.phase = Phase::Listening;
            }
            Message::Said(text) => {
                live.heard = None;
                let line = *live.said.get_or_insert_with(|| {
                    call.lines.push(Line {
                        value: CallLine {
                            who: "live".into(),
                            name: host_name,
                            text: String::new(),
                            status: LineStatus::Said,
                        },
                        note: None,
                    });
                    call.lines.len() - 1
                });
                call.lines[line].value.text.push_str(&text);
                call.phase = Phase::Answering;
            }
            Message::TurnDone => {
                live.heard = None;
                live.said = None;
                call.phase = Phase::Listening;
            }
            Message::Interrupted => {
                live.said = None;
                call.phase = Phase::Listening;
            }
            Message::Ask {
                member,
                request,
                reply,
            } => {
                self.call_live_ask(member, request, reply, cx);
                return;
            }
        }
        cx.notify();
    }

    fn call_live_ask(
        &mut self,
        member: String,
        request: String,
        reply: std::sync::mpsc::Sender<Result<String, String>>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call.as_mut() else {
            let _ = reply.send(Err("The call ended.".into()));
            return;
        };
        let Some(group) = self.roster.get(&call.group).cloned() else {
            let _ = reply.send(Err("The group is gone.".into()));
            return;
        };
        let Some(found) = call
            .members
            .iter()
            .find(|candidate| candidate.id == member)
            .cloned()
        else {
            let _ = reply.send(Err("That agent left the call.".into()));
            return;
        };
        if call
            .live
            .as_ref()
            .is_some_and(|live| live.asks.iter().any(|ask| ask.member == member))
        {
            let _ = reply.send(Err(format!(
                "{} is still on the previous request; wait for its answer.",
                found.name
            )));
            return;
        }
        let others: Vec<String> = call
            .members
            .iter()
            .filter(|other| other.id != member)
            .map(|other| other.name.clone())
            .collect();
        let prompt = plan::member_prompt(
            &group.label(),
            group.group.as_ref().and_then(|group| group.goal.as_deref()),
            &others,
            &request,
        );
        let id = format!("live-{}-{}", call.group, mono::new_id());
        call.lines.push(Line {
            value: CallLine {
                who: member.clone(),
                name: found.name.clone(),
                text: String::new(),
                status: LineStatus::Thinking,
            },
            note: None,
        });
        let line = call.lines.len() - 1;
        let group_id = call.group.clone();
        match self.call_send_member(&group_id, &member, prompt, id.clone(), cx) {
            Ok(()) => {
                if let Some(live) = self.call.as_mut().and_then(|call| call.live.as_mut()) {
                    live.asks.push(Ask {
                        request: id,
                        member,
                        name: found.name,
                        line,
                        reply,
                    });
                }
            }
            Err(why) => {
                if let Some(call) = self.call.as_mut() {
                    call.lines[line].value.status = LineStatus::Busy;
                    call.lines[line].note = Some(why.clone());
                }
                let _ = reply.send(Err(why));
            }
        }
        cx.notify();
    }

    /// A member's seat changed: hand back the answers that finished.
    pub(in crate::monos) fn call_live_progress(&mut self, member: &str, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        let Some(live) = call.live.as_ref() else {
            return;
        };
        let group = call.group.clone();
        let mut finished = Vec::new();
        for (at, ask) in live
            .asks
            .iter()
            .enumerate()
            .filter(|(_, ask)| ask.member == member)
        {
            let failed = self.call_request_failed(&ask.request);
            let seat = self
                .groups
                .seats
                .get(&(group.clone(), ask.member.clone()))
                .and_then(|seat| seat.view.as_ref())
                .map(|view| {
                    let view = view.read(cx);
                    (native::blocks(view.mono_rows(cx)), view.busy)
                });
            let Some((blocks, busy)) = seat else {
                if failed {
                    finished.push((at, Err("It could not answer.".to_owned())));
                }
                continue;
            };
            let outcome = round_answer(
                SeatView {
                    blocks: &blocks,
                    busy,
                },
                &ask.request,
            );
            let reply = call::member_reply(&blocks, &ask.request);
            if let Some(error) = outcome.as_ref().and_then(|answer| answer.error.clone()) {
                finished.push((at, Err(error)));
            } else if failed {
                finished.push((at, Err("It could not answer.".to_owned())));
            } else if outcome.is_some() {
                finished.push((at, Ok(reply)));
            }
        }
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Some(live) = call.live.as_mut() else {
            return;
        };
        for (at, result) in finished.into_iter().rev() {
            let ask = live.asks.remove(at);
            let line = &mut call.lines[ask.line];
            match &result {
                Ok(text) => {
                    line.value.text = text.trim().to_owned();
                    line.value.status = LineStatus::Said;
                    call.earlier
                        .push((ask.name.clone(), text.trim().to_owned()));
                }
                Err(why) => {
                    line.value.status = LineStatus::Failed;
                    line.note = Some(why.clone());
                }
            }
            let _ = ask.reply.send(result);
        }
        cx.notify();
    }

    /// Ends the live side of a call: the session closes off the interface
    /// thread, and the members still working are told to stop.
    pub(in crate::monos) fn call_live_end(
        &mut self,
        live: LiveCall,
        group: &str,
        cx: &mut Context<Self>,
    ) {
        let LiveCall { session, asks, .. } = live;
        for ask in asks {
            let _ = ask.reply.send(Err("The call ended.".into()));
            self.cancel_call_seat(group, &ask.member, &ask.request, cx);
        }
        if let Some(session) = session {
            cx.background_executor()
                .spawn(async move { session.hang_up() })
                .detach();
        }
    }

    pub(in crate::monos) fn call_live_asking(&self, member: &str) -> bool {
        self.call
            .as_ref()
            .and_then(|call| call.live.as_ref())
            .is_some_and(|live| live.asks.iter().any(|ask| ask.member == member))
    }

    #[cfg(test)]
    pub(in crate::monos) fn call_test_live(
        &mut self,
        group: &Mono,
        provider: Provider,
        cx: &mut Context<Self>,
    ) -> u64 {
        self.ensure_call(group, cx);
        let call = self.call.as_mut().unwrap();
        call.epoch = next_generation();
        call.phase = Phase::Starting;
        call.started_at = now_ms();
        call.live = Some(LiveCall {
            provider,
            session: None,
            _duplex: None,
            asks: Vec::new(),
            heard: None,
            said: None,
            _messages: None,
        });
        cx.notify();
        call.epoch
    }

    #[cfg(test)]
    pub(in crate::monos) fn call_test_live_asks(&self) -> Vec<(String, String)> {
        self.call
            .as_ref()
            .and_then(|call| call.live.as_ref())
            .map(|live| {
                live.asks
                    .iter()
                    .map(|ask| (ask.member.clone(), ask.request.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(in crate::monos) fn call_test_live_listening(&self) -> bool {
        self.call
            .as_ref()
            .is_some_and(|call| call.live.is_some() && call.phase == Phase::Listening)
    }

    #[cfg(test)]
    pub(in crate::monos) fn call_test_live_host(
        &self,
    ) -> (LiveHost, async_channel::Receiver<Message>) {
        let (messages, incoming) = async_channel::unbounded();
        let members = self
            .call
            .as_ref()
            .map(|call| call.members.clone())
            .unwrap_or_default();
        (
            LiveHost {
                messages,
                duplex: Mutex::new(None),
                rate: gemini::OUTPUT_RATE,
                members,
            },
            incoming,
        )
    }
}
