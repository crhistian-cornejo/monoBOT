//! Groups of agents (App.tsx's group parts and agentApp.ts handleGroup): a
//! message typed in a group's chat goes to one member (@Name) or a round,
//! each member answers from its own seat in the group, its answer posts in
//! the chat under its name, and the lead has the last word. The lead brings
//! members in with group.ask, and anyone in the group asks the user for a
//! path or link with group.access. The decisions are core's
//! (monocode_core::mono::groups); this keeps the seats, rounds and posts.
use super::*;
use monocode_core::{
    mono::{
        bridge::Call,
        files,
        groups::{
            Access, Block, GroupMono,
            app::{self as group_app, NOT_A_GROUP},
            brief::{Member, askable_members, group_lead},
            group::{access_answer_text, answer_access_request, group_message, groups_of},
            learning::{self, LearnedMarks},
            mentions::Named,
            native::{self, AskResult, Mark},
            replies::{self, DroppedRequests, finished_group_replies},
            round::{
                GroupRound, GroupRoute, RoundStatus, SeatView, round_answer, round_prompt,
                round_status, route_group_message,
            },
        },
    },
    time::now_ms,
    transcript::Row,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{Arc, atomic::AtomicBool},
};

/// How long the small model may take to read an exchange.
const LEARN_TIMEOUT: Duration = Duration::from_secs(45);

/// A member's own conversation in a group.
pub(super) struct Seat {
    /// None while it loads.
    pub(super) view: Option<Entity<Conversation>>,
    /// What the group asked it and it has not been sent yet, oldest first.
    queue: VecDeque<SeatAsk>,
    /// An ask on its way in, while the group's memory is read.
    sending: Option<String>,
}

struct SeatAsk {
    request: String,
    text: String,
    quick: bool,
}

/// A group.ask whose answer comes back to the lead.
struct AskBack {
    group: String,
    member: String,
    name: String,
    /// The lead's turn it asked in: a turn's answers come back together.
    turn: u64,
    wait: bool,
    prompt: String,
}

#[derive(Default)]
pub(super) struct Groups {
    /// By (group id, member id).
    pub(super) seats: HashMap<(String, String), Seat>,
    /// Each seat's conversation, for the app calls it makes.
    seat_sessions: HashMap<i64, (String, String)>,
    rounds: Vec<GroupRound>,
    /// Requests that could not reach their member.
    failed: HashSet<String>,
    /// Requests cancelled while their group's memory is being read.
    cancelled: HashSet<String>,
    /// Members' posts waiting for their group's chat to be free.
    posts: HashMap<String, VecDeque<(String, Vec<Row>)>>,
    posting: HashSet<String>,
    asks: HashMap<String, AskBack>,
    /// Answers to group.ask held until the rest of their turn's are in.
    answered: Vec<(AskBack, AskResult)>,
    dropped: Option<DroppedRequests>,
    learned: Option<LearnedMarks>,
    learning: bool,
    learn_failed: HashSet<String>,
}

impl Monos {
    /// A group's Mono with the Principal leading it, as groups read it.
    fn led_group(&self, id: &str) -> Option<GroupMono> {
        native::group_monos(&self.roster.monos)
            .into_iter()
            .find(|group| group.id == id)
    }

    /// The id its requests are named after: its chat's.
    fn group_key(&self, id: &str) -> Option<String> {
        self.roster.get(id)?.session_id.map(|id| id.to_string())
    }

    /// What a Mono's groups add to its chat, or to its seat in `seat`.
    pub(super) fn group_context(&self, mono: &Mono, seat: Option<&str>) -> native::Context {
        let seat = seat.and_then(|id| self.roster.get(id));
        native::Context::of(mono, &self.roster.monos, seat)
    }

    /// The group and member whose seat a session is.
    pub(super) fn seat_owner(&self, session: i64) -> Option<(String, String)> {
        self.groups.seat_sessions.get(&session).cloned()
    }

    fn dropped(&mut self) -> &mut DroppedRequests {
        let store = self.store.clone();
        self.groups.dropped.get_or_insert_with(|| {
            locked(&store)
                .map(|db| native::load_dropped(&db))
                .unwrap_or_default()
        })
    }

    fn learned(&mut self) -> &mut LearnedMarks {
        let store = self.store.clone();
        self.groups.learned.get_or_insert_with(|| {
            locked(&store)
                .map(|db| native::load_learned(&db))
                .unwrap_or_default()
        })
    }

    /// A message the user wrote in a group's chat (`directGroupAsk`): to one
    /// member with @Name, or to every member before the lead. False when the
    /// lead takes it as it is.
    pub(super) fn group_submitted(
        &mut self,
        source: &str,
        text: &str,
        attachments: &[PathBuf],
        cx: &mut Context<Self>,
    ) -> bool {
        if !attachments.is_empty() {
            return false;
        }
        let (Some(led), Some(chat), Some(key)) = (
            self.led_group(source),
            self.chats.get(source).cloned(),
            self.group_key(source),
        ) else {
            return false;
        };
        if chat.read(cx).busy {
            return false;
        }
        let roster = native::members(&self.roster.monos);
        let members: Vec<Member> = askable_members(&led.group, &roster)
            .into_iter()
            .cloned()
            .collect();
        let lead = group_lead(&led.group, &roster).cloned();
        let now = now_ms();
        let Some(route) = route_group_message(
            &key,
            text,
            &members,
            lead.as_ref().map(|lead| lead as &dyn Named),
            &mono::new_id(),
            now as i64,
        ) else {
            return false;
        };
        let user_request = match &route {
            GroupRoute::Direct { user_request, .. } | GroupRoute::Round { user_request, .. } => {
                user_request.clone()
            }
        };
        let rows = native::ask_rows(&user_request, text.trim(), now);
        if !chat.update(cx, |chat, cx| chat.mono_append(rows, cx)) {
            return false;
        }
        match route {
            GroupRoute::Direct {
                member_id,
                message,
                member_request,
                ..
            } => self.send_to_member(source, &member_id, message, member_request, false, cx),
            GroupRoute::Round { round, .. } => {
                for ask in &round.asks {
                    self.send_to_member(
                        source,
                        &ask.member_id,
                        round.text.clone(),
                        ask.request.clone(),
                        round.quick,
                        cx,
                    );
                }
                self.groups.rounds.push(round);
            }
        }
        self.changed(cx);
        true
    }

    /// One member's part of a group's message, in its seat.
    pub(super) fn send_to_member(
        &mut self,
        group: &str,
        member: &str,
        text: String,
        request: String,
        quick: bool,
        cx: &mut Context<Self>,
    ) {
        let key = (group.to_owned(), member.to_owned());
        if !self.ensure_seat(&key, cx) {
            self.groups.failed.insert(request);
            return;
        }
        if let Some(seat) = self.groups.seats.get_mut(&key) {
            seat.queue.push_back(SeatAsk {
                request,
                text,
                quick,
            });
        }
        self.pump_seat(&key, cx);
    }

    /// Calls skip a busy member instead of inserting speech into its turn.
    pub(super) fn call_send_member(
        &mut self,
        group: &str,
        member: &str,
        text: String,
        request: String,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let key = (group.to_owned(), member.to_owned());
        if !self.ensure_seat(&key, cx) {
            return Err("The member or group no longer exists".into());
        }
        let seat = self.groups.seats.get_mut(&key).expect("seat was created");
        if seat.sending.is_some()
            || !seat.queue.is_empty()
            || seat
                .view
                .as_ref()
                .is_some_and(|view| view.read(cx).mono_call_blocked(cx))
        {
            return Err("The member is busy".into());
        }
        seat.queue.push_back(SeatAsk {
            request,
            text,
            quick: false,
        });
        self.pump_seat(&key, cx);
        Ok(())
    }

    pub(super) fn call_request_failed(&self, request: &str) -> bool {
        self.groups.failed.contains(request)
    }

    #[cfg(test)]
    pub(super) fn call_test_queued(&self, group: &str, member: &str, request: &str) -> bool {
        self.groups
            .seats
            .get(&(group.to_owned(), member.to_owned()))
            .is_some_and(|seat| seat.queue.iter().any(|ask| ask.request == request))
    }

    /// Cancels only this call request, not another turn sharing the seat.
    pub(super) fn cancel_call_seat(
        &mut self,
        group: &str,
        member: &str,
        request: &str,
        cx: &mut Context<Self>,
    ) {
        let key = (group.to_owned(), member.to_owned());
        let Some(seat) = self.groups.seats.get_mut(&key) else {
            return;
        };
        let queued = seat.queue.len();
        seat.queue.retain(|ask| ask.request != request);
        if seat.queue.len() != queued {
            return;
        }
        if seat.sending.as_deref() == Some(request) {
            self.groups.cancelled.insert(request.to_owned());
            return;
        }
        let Some(view) = seat.view.clone() else {
            return;
        };
        let current = {
            let view = view.read(cx);
            view.busy
                && native::blocks(view.mono_rows(cx))
                    .iter()
                    .rev()
                    .find(|block| block.is_user())
                    .is_some_and(|block| block.app_request_id.as_deref() == Some(request))
        };
        if current {
            view.update(cx, |view, cx| view.mono_stop(cx));
        }
    }

    /// A member's seat in a group (`ensureGroupSeat`): its own conversation
    /// there, made the first time it is asked, with the model, settings and
    /// permissions of its chat. False when the group or member is gone.
    pub(super) fn ensure_seat(&mut self, key: &(String, String), cx: &mut Context<Self>) -> bool {
        if self.groups.seats.contains_key(key) {
            return true;
        }
        let (Some(group), Some(member)) = (
            self.roster.get(&key.0).cloned(),
            self.roster.get(&key.1).cloned(),
        ) else {
            return false;
        };
        self.groups.seats.insert(
            key.clone(),
            Seat {
                view: None,
                queue: VecDeque::new(),
                sending: None,
            },
        );
        let own = group.seats.get(&member.id).copied();
        let desktop = group
            .group
            .as_ref()
            .and_then(|g| g.seats.get(&member.id).cloned());
        let store = self.store.clone();
        let prefs = self.prefs.clone();
        let title = format!("{} · {}", member.label(), group.label());
        let work = cx.background_executor().spawn(async move {
            let db = locked(&store)?;
            let existing = own.or_else(|| {
                desktop
                    .as_deref()
                    .and_then(|id| db.resolve_existing_session_id(id))
            });
            if let Some(summary) = existing.and_then(|id| db.mono_session(id).ok().flatten()) {
                return Ok::<_, String>((summary, None));
            }
            // The member's chat is what its seat follows.
            let chat = member
                .session_id
                .and_then(|id| db.mono_session(id).ok().flatten());
            let provider = chat.as_ref().map_or_else(
                || {
                    prefs
                        .default_provider
                        .as_deref()
                        .and_then(Kind::parse)
                        .unwrap_or(Kind::Codex)
                        .id()
                        .to_owned()
                },
                |chat| chat.provider.clone(),
            );
            let cwd = chat.as_ref().map_or_else(
                || {
                    std::env::var("HOME")
                        .or_else(|_| std::env::var("USERPROFILE"))
                        .unwrap_or_else(|_| ".".into())
                },
                |chat| chat.cwd.clone(),
            );
            let mut settings = chat
                .as_ref()
                .and_then(|chat| db.agent_settings(chat.id).ok().flatten())
                .unwrap_or_else(|| Settings {
                    access: AccessMode::Auto,
                    model: prefs.default_models.get(&provider).cloned(),
                    effort: Some("low".into()),
                    ..Default::default()
                });
            settings.revision = 0;
            settings.turns.clear();
            settings.handoff = None;
            settings.pending_provider_switch = None;
            let mut summary = db.create(&provider, &cwd)?;
            summary.title = title;
            db.title(summary.id, &summary.title)?;
            db.configure_agent(summary.id, &provider, &settings)?;
            Ok((summary, Some(settings)))
        });
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| this.seat_ready(key, result, cx));
        })
        .detach();
        true
    }

    fn seat_ready(
        &mut self,
        key: (String, String),
        result: Result<(Session, Option<Settings>), String>,
        cx: &mut Context<Self>,
    ) {
        let member = self.roster.get(&key.1).cloned();
        let (summary, settings, member) = match (result, member) {
            (Ok((summary, settings)), Some(member)) => (summary, settings, member),
            (result, _) => {
                if let Err(error) = result {
                    self.error = Some(error);
                }
                if let Some(seat) = self.groups.seats.remove(&key) {
                    self.groups
                        .failed
                        .extend(seat.queue.into_iter().map(|ask| ask.request));
                }
                self.check_groups(cx);
                return;
            }
        };
        let session = summary.id;
        let view = cx.new(|cx| Conversation::new(summary, self.store.clone(), cx));
        let root = self.root.clone();
        let env = self
            .bridge
            .as_ref()
            .map(|b| b.environment(session))
            .unwrap_or_default();
        let context = self.group_context(&member, Some(&key.0));
        view.update(cx, |view, cx| {
            view.configure_mono(member, root, env, cx);
            if let Some(settings) = settings {
                let config = view.settings.mono.take();
                let env = std::mem::take(&mut view.settings.environment);
                view.install_transfer(settings, None, cx);
                view.settings.mono = config;
                view.settings.environment = env;
            }
            view.set_mono_groups(context, cx);
            view.load(cx);
        });
        let source = key.clone();
        self.observations
            .push(cx.subscribe(&view, move |this, _, _: &SessionChanged, cx| {
                this.seat_changed(&source, cx)
            }));
        let transcript = view.read(cx).transcript.clone();
        let source = key.clone();
        self.observations
            .push(cx.observe(&transcript, move |this, _, cx| {
                this.call_seat_changed(&source, cx);
            }));
        if let Some(seat) = self.groups.seats.get_mut(&key) {
            seat.view = Some(view);
        }
        self.groups.seat_sessions.insert(session, key.clone());
        if !self.roster.hidden_sessions.contains(&session) {
            self.roster.hidden_sessions.push(session);
        }
        if let Some(group) = self.roster.get_mut(&key.0)
            && group.seats.get(&key.1) != Some(&session)
        {
            group.seats.insert(key.1.clone(), session);
        }
        self.save(cx);
        self.pump_seat(&key, cx);
    }

    fn seat_changed(&mut self, key: &(String, String), cx: &mut Context<Self>) {
        self.call_seat_changed(key, cx);
        let idle = self
            .groups
            .seats
            .get(key)
            .and_then(|seat| seat.view.as_ref())
            .is_some_and(|view| !view.read(cx).busy);
        if idle {
            self.pump_seat(key, cx);
            self.check_groups(cx);
        }
    }

    /// Sends a seat its next ask once it is free, with the project's memory.
    fn pump_seat(&mut self, key: &(String, String), cx: &mut Context<Self>) {
        let Some(seat) = self.groups.seats.get_mut(key) else {
            return;
        };
        let Some(view) = seat.view.clone() else {
            return;
        };
        if seat.sending.is_some() || seat.queue.is_empty() {
            return;
        }
        if !view.read(cx).ready_for_external(cx) {
            // Its history may have been set aside; it settles and comes back.
            if !view.read(cx).busy {
                view.update(cx, |view, cx| view.load(cx));
            }
            return;
        }
        let Some(ask) = seat.queue.pop_front() else {
            return;
        };
        seat.sending = Some(ask.request.clone());
        let root = self.root.clone();
        let group = key.0.clone();
        let memory = cx.background_executor().spawn(async move {
            files::read(&root, &group, None, "MEMORY.md")
                .ok()
                .and_then(|file| file.text)
                .unwrap_or_default()
        });
        let key = key.clone();
        cx.spawn(async move |this, cx| {
            let memory = memory.await;
            let _ = this.update(cx, |this, cx| this.deliver(key, ask, memory, cx));
        })
        .detach();
    }

    fn deliver(
        &mut self,
        key: (String, String),
        ask: SeatAsk,
        memory: String,
        cx: &mut Context<Self>,
    ) {
        if self.groups.cancelled.remove(&ask.request) {
            if let Some(seat) = self.groups.seats.get_mut(&key) {
                seat.sending = None;
            }
            self.pump_seat(&key, cx);
            return;
        }
        let led = self.led_group(&key.0);
        let Some(seat) = self.groups.seats.get_mut(&key) else {
            self.groups.failed.insert(ask.request);
            return;
        };
        seat.sending = None;
        let Some(view) = seat.view.clone() else {
            return;
        };
        let Some(led) = led else {
            self.groups.failed.insert(ask.request);
            return;
        };
        if !view.read(cx).ready_for_external(cx) {
            seat.queue.push_front(ask);
            return;
        }
        // A seat keeps following its member's chat: the same provider and
        // model, whichever the user picked there since.
        if let Some(own) = self.chats.get(&key.1) {
            let own = own.read(cx);
            let (provider, model, values) = (
                own.summary.provider.clone(),
                own.mono_model(),
                own.settings.model_settings.clone(),
            );
            let differs = {
                let current = view.read(cx);
                current.summary.provider != provider || current.mono_model() != model
            };
            if differs && let Some(kind) = Kind::parse(&provider) {
                let _ = view.update(cx, |view, cx| view.mono_config(kind, model, values, cx));
            }
        }
        let text = group_message(
            &led.name,
            led.group.goal.as_deref(),
            &ask.text,
            Some(&led.group),
            Some(&memory),
            ask.quick,
        );
        let now = now_ms();
        let mark = Mark::GroupRequest(ask.request.clone()).row(Some(now));
        if !view.update(cx, |view, cx| view.mono_append(vec![mark], cx)) {
            seat.queue.push_front(ask);
            return;
        }
        let message = Message {
            id: mono::new_id(),
            text,
            sent_at: Some(now),
            internal: false,
            error: None,
            attachments: Vec::new(),
        };
        if !view.update(cx, |view, cx| view.mono_dispatch(&message, cx)) {
            self.groups.failed.insert(ask.request);
        }
        self.check_groups(cx);
    }

    /// Each loaded seat's conversation as the desktop's blocks, and whether
    /// it is answering now.
    pub(super) fn seat_blocks(&self, cx: &App) -> HashMap<(String, String), (Vec<Block>, bool)> {
        self.groups
            .seats
            .iter()
            .filter_map(|(key, seat)| {
                let view = seat.view.as_ref()?.read(cx);
                Some((key.clone(), (native::blocks(view.mono_rows(cx)), view.busy)))
            })
            .collect()
    }

    /// Posts members' finished answers, settles rounds and asks, and lets
    /// a settled exchange teach the group (App.tsx's group effects).
    pub(super) fn check_groups(&mut self, cx: &mut Context<Self>) {
        if !self.roster.monos.iter().any(Mono::is_group) {
            return;
        }
        self.call_progress(cx);
        let seats = self.seat_blocks(cx);
        let now = now_ms();
        self.post_replies(&seats, now, cx);
        self.flush_posts(cx);
        self.settle_rounds(&seats, now, cx);
        self.settle_asks(&seats, cx);
        self.learn(&seats, cx);
    }

    /// A group's chat reads like a team's: each member's answer posts there
    /// under its own name as soon as it is done.
    fn post_replies(
        &mut self,
        seats: &HashMap<(String, String), (Vec<Block>, bool)>,
        now: u64,
        cx: &mut Context<Self>,
    ) {
        let dropped = self.dropped().clone();
        for group in native::group_monos(&self.roster.monos) {
            let (Some(chat), Some(session)) = (self.chats.get(&group.id), &group.session_id) else {
                continue;
            };
            let mut posted = native::posted_requests(chat.read(cx).mono_rows(cx));
            posted.extend(self.groups.posting.iter().cloned());
            for member_id in &group.group.members {
                let key = (group.id.clone(), member_id.clone());
                let (Some((blocks, busy)), Some(member)) =
                    (seats.get(&key), self.roster.get(member_id))
                else {
                    continue;
                };
                let (harness, model) = self
                    .groups
                    .seats
                    .get(&key)
                    .and_then(|seat| seat.view.as_ref())
                    .map(|view| {
                        let view = view.read(cx);
                        (
                            view.summary.provider.clone(),
                            view.mono_model_label().to_string(),
                        )
                    })
                    .unwrap_or_default();
                let author = native::post_author(member, Some(&harness), Some(&model));
                for reply in finished_group_replies(blocks, *busy, session, 0, &dropped) {
                    if !posted.insert(reply.request.clone()) {
                        continue;
                    }
                    let Some(post) =
                        replies::group_member_post(&author, &reply, &mono::new_id(), now as i64)
                            .group_member
                    else {
                        continue;
                    };
                    self.groups.posting.insert(reply.request.clone());
                    self.groups
                        .posts
                        .entry(group.id.clone())
                        .or_default()
                        .push_back((
                            reply.request.clone(),
                            native::post_rows(&post, &reply.text, now),
                        ));
                }
            }
        }
    }

    /// Writes waiting posts into their chats once each is free.
    fn flush_posts(&mut self, cx: &mut Context<Self>) {
        let groups: Vec<String> = self.groups.posts.keys().cloned().collect();
        for group in groups {
            let Some(chat) = self.chats.get(&group).cloned() else {
                continue;
            };
            let Some(queue) = self.groups.posts.get_mut(&group) else {
                continue;
            };
            while let Some((request, rows)) = queue.front().cloned() {
                if !chat.update(cx, |chat, cx| chat.mono_append(rows, cx)) {
                    break;
                }
                queue.pop_front();
                self.groups.posting.remove(&request);
            }
            if queue.is_empty() {
                self.groups.posts.remove(&group);
            }
        }
    }

    /// Hands each finished round to the lead, once, for the last word. An
    /// errand someone did ends there: the others stop and nothing reaches
    /// the lead, which reads the answer with the user's next message.
    fn settle_rounds(
        &mut self,
        seats: &HashMap<(String, String), (Vec<Block>, bool)>,
        now: u64,
        cx: &mut Context<Self>,
    ) {
        let rounds = std::mem::take(&mut self.groups.rounds);
        let mut waiting = Vec::new();
        for mut round in rounds {
            let Some(group) = self
                .roster
                .monos
                .iter()
                .find(|m| {
                    m.session_id.map(|id| id.to_string()).as_deref()
                        == Some(&round.group_session_id)
                })
                .map(|m| m.id.clone())
            else {
                continue;
            };
            round.failed = round
                .asks
                .iter()
                .filter(|ask| self.groups.failed.contains(&ask.request))
                .map(|ask| ask.request.clone())
                .collect();
            let status = round_status(
                &round,
                |member| {
                    seats
                        .get(&(group.clone(), member.to_owned()))
                        .map(|(blocks, busy)| SeatView {
                            blocks,
                            busy: *busy,
                        })
                },
                now as i64,
            );
            match status {
                RoundStatus::Waiting => waiting.push(round),
                RoundStatus::Done { stop, .. } => {
                    if self.dropped().drop_requests(&stop) {
                        let dropped = self.dropped().clone();
                        let store = self.store.clone();
                        cx.background_executor()
                            .spawn(async move {
                                let _ = locked(&store)
                                    .and_then(|db| native::save_dropped(&db, &dropped));
                            })
                            .detach();
                    }
                    for request in &stop {
                        let Some(ask) = round.asks.iter().find(|ask| &ask.request == request)
                        else {
                            continue;
                        };
                        if let Some(seat) = self
                            .groups
                            .seats
                            .get_mut(&(group.clone(), ask.member_id.clone()))
                        {
                            seat.queue.retain(|queued| &queued.request != request);
                            if let Some(view) = &seat.view {
                                view.update(cx, |view, cx| view.stop_if_busy(cx));
                            }
                        }
                    }
                }
                RoundStatus::Lead(answers) => {
                    if self.chats.get(&group).is_none_or(|chat| chat.read(cx).busy) {
                        waiting.push(round);
                        continue;
                    }
                    self.enqueue(
                        &group,
                        round_prompt(&round.text, &answers),
                        Vec::new(),
                        true,
                        cx,
                    );
                }
            }
        }
        // Rounds may have started while these settled.
        waiting.append(&mut self.groups.rounds);
        self.groups.rounds = waiting;
    }

    /// group.ask answers: to the call waiting for one, else back to the
    /// lead together with the rest of the turn's.
    fn settle_asks(
        &mut self,
        seats: &HashMap<(String, String), (Vec<Block>, bool)>,
        cx: &mut Context<Self>,
    ) {
        let mut done = Vec::new();
        for (request, ask) in &self.groups.asks {
            let key = (ask.group.clone(), ask.member.clone());
            let answer = if self.groups.failed.contains(request) {
                Some((String::new(), Some("could not take the message".to_owned())))
            } else {
                seats.get(&key).and_then(|(blocks, busy)| {
                    round_answer(
                        SeatView {
                            blocks,
                            busy: *busy,
                        },
                        request,
                    )
                    .map(|answer| (answer.text, answer.error))
                })
            };
            if let Some(answer) = answer {
                done.push((request.clone(), answer));
            }
        }
        for (request, (text, error)) in done {
            let Some(ask) = self.groups.asks.remove(&request) else {
                continue;
            };
            if ask.wait {
                let reply = match &error {
                    Some(error) => format!("Couldn't answer: {error}"),
                    None => text,
                };
                self.route_answered(&request, reply, cx);
                continue;
            }
            let session = self
                .groups
                .seats
                .get(&(ask.group.clone(), ask.member.clone()))
                .and_then(|seat| seat.view.as_ref())
                .map(|view| view.read(cx).summary.clone());
            let result = AskResult::new(
                &session
                    .as_ref()
                    .map(|s| s.id.to_string())
                    .unwrap_or_default(),
                &session.map_or_else(|| ask.name.clone(), |s| s.title),
                &ask.prompt,
                &text,
                error.as_deref(),
            );
            self.groups.answered.push((ask, result));
        }
        // A turn's answers come back together, once the lead is free.
        let mut turns: Vec<(String, u64)> = self
            .groups
            .answered
            .iter()
            .map(|(ask, _)| (ask.group.clone(), ask.turn))
            .collect();
        turns.sort();
        turns.dedup();
        for (group, turn) in turns {
            let open = self
                .groups
                .asks
                .values()
                .any(|ask| !ask.wait && ask.group == group && ask.turn == turn);
            if open || self.chats.get(&group).is_none_or(|chat| chat.read(cx).busy) {
                continue;
            }
            let (ready, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.groups.answered)
                .into_iter()
                .partition(|(ask, _)| ask.group == group && ask.turn == turn);
            self.groups.answered = rest;
            let results: Vec<AskResult> = ready.into_iter().map(|(_, result)| result).collect();
            if !results.is_empty() {
                self.enqueue(
                    &group,
                    native::ask_completion(&results),
                    Vec::new(),
                    true,
                    cx,
                );
            }
        }
    }

    /// What a group's chat settles joins the project's memory on its own
    /// (useGroupLearning): one group at a time, once its chat and members
    /// are idle, read by the cheapest model its harness has.
    fn learn(
        &mut self,
        seats: &HashMap<(String, String), (Vec<Block>, bool)>,
        cx: &mut Context<Self>,
    ) {
        if self.groups.learning || cfg!(test) {
            return;
        }
        let roster = native::members(&self.roster.monos);
        for group in native::group_monos(&self.roster.monos) {
            let Some(chat) = self.chats.get(&group.id).cloned() else {
                continue;
            };
            if chat.read(cx).busy
                || seats
                    .iter()
                    .any(|((owner, _), (_, busy))| owner == &group.id && *busy)
            {
                continue;
            }
            let chat_id = chat.read(cx).summary.id.to_string();
            let mark = self.learned().learned_up_to(&chat_id).map(String::from);
            let lead =
                group_lead(&group.group, &roster).map_or("Lead".to_owned(), |m| m.name.clone());
            let blocks = native::blocks(chat.read(cx).mono_rows(cx));
            let Some(exchange) = learning::pending_exchange(&blocks, mark.as_deref(), &lead) else {
                continue;
            };
            if self.groups.learn_failed.contains(&exchange.last_id) {
                continue;
            }
            let text = chat.read(cx).background_text_model(cx);
            let Some(binary) = monocode_core::provider::resolve(text.kind) else {
                return;
            };
            self.groups.learning = true;
            let root = self.root.clone();
            let id = group.id.clone();
            let name = group.name.clone();
            let goal = group.group.goal.clone();
            let cancel = Arc::new(AtomicBool::new(false));
            let stop = cancel.clone();
            let last = exchange.last_id.clone();
            let work = cx.background_executor().spawn(async move {
                let current = files::read(&root, &id, None, "MEMORY.md")?;
                let memory = current.text.clone().unwrap_or_default();
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_else(|_| ".".into());
                let output = monocode_core::provider::run_text(
                    monocode_core::provider::Request {
                        kind: text.kind,
                        binary,
                        cwd: home.into(),
                        prompt: learning::learning_prompt(
                            &name,
                            goal.as_deref(),
                            &memory,
                            &exchange,
                        ),
                        provider_id: None,
                        model: text.model,
                        settings: Settings {
                            account: text.account,
                            ..Default::default()
                        },
                    },
                    cancel,
                )?;
                let facts = learning::parse_learned(&output, &memory);
                if !facts.is_empty() {
                    let next = monocode_core::mono::groups::project::memory_seed(
                        &memory,
                        &facts.join("\n"),
                        &monocode_core::mono::memory::date(),
                    )?;
                    if next != memory {
                        files::save(&root, &id, None, "MEMORY.md", &next, Some(&current.hash))?;
                    }
                }
                Ok::<_, String>(())
            });
            cx.spawn(async move |_, cx| {
                cx.background_executor().timer(LEARN_TIMEOUT).await;
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
            })
            .detach();
            cx.spawn(async move |this, cx| {
                let result = work.await;
                let _ = this.update(cx, |this, cx| {
                    this.groups.learning = false;
                    if result.is_err() {
                        this.groups.learn_failed.insert(last);
                        return;
                    }
                    this.learned().mark_learned(&chat_id, &last);
                    let marks = this.learned().clone();
                    let store = this.store.clone();
                    cx.background_executor()
                        .spawn(async move {
                            let _ = locked(&store).and_then(|db| native::save_learned(&db, &marks));
                        })
                        .detach();
                });
            })
            .detach();
            return;
        }
    }

    /// The group actions (handleGroup): the lead lists and asks its members;
    /// anyone in a group asks the user for access. None when answered.
    pub(super) fn group_call(
        &mut self,
        call: Call,
        mono: &Mono,
        action: &str,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Option<Call> {
        let result = match action {
            "group.access" => self.group_access(&call, mono, input, cx),
            "group.members" | "group.ask" => {
                let Some(led) = self.led_group(&mono.id).filter(|_| mono.is_group()) else {
                    self.finish_call(call, Err(NOT_A_GROUP.into()), cx);
                    return None;
                };
                if action == "group.ask" {
                    self.group_ask(call, mono, &led, input, cx);
                    return None;
                }
                let roster = native::members(&self.roster.monos);
                Ok(group_app::members_reply(&askable_members(
                    &led.group, &roster,
                )))
            }
            _ => return Some(call),
        };
        self.finish_call(call, result, cx);
        None
    }

    /// group.ask: a member answers from its seat; the answer comes back to
    /// the lead with the turn's other answers, or to this call when it waits.
    fn group_ask(
        &mut self,
        call: Call,
        mono: &Mono,
        led: &GroupMono,
        input: &Value,
        cx: &mut Context<Self>,
    ) {
        let roster = native::members(&self.roster.monos);
        let askable = askable_members(&led.group, &roster);
        let checked = self
            .group_key(&mono.id)
            .ok_or_else(|| NOT_A_GROUP.to_owned())
            .and_then(|key| group_app::group_ask(&askable, &key, &call.request_id, input));
        let ask = match checked {
            Ok(ask) => ask,
            Err(error) => {
                self.finish_call(call, Err(error), cx);
                return;
            }
        };
        let (member, name) = (ask.member.id.clone(), ask.member.name.clone());
        // As a session the app sends to: a member still answering is busy.
        let busy = self
            .groups
            .seats
            .get(&(mono.id.clone(), member.clone()))
            .is_some_and(|seat| {
                seat.sending.is_some()
                    || !seat.queue.is_empty()
                    || seat.view.as_ref().is_some_and(|view| view.read(cx).busy)
            });
        if busy {
            self.finish_call(
                call,
                Err("Session is busy; try again when it finishes".into()),
                cx,
            );
            return;
        }
        let turn = self
            .chats
            .get(&mono.id)
            .map(|view| view.read(cx).mono_turn)
            .unwrap_or(0);
        self.groups.asks.insert(
            ask.app_request_id.clone(),
            AskBack {
                group: mono.id.clone(),
                member: member.clone(),
                name: name.clone(),
                turn,
                wait: ask.wait,
                prompt: ask.message.clone(),
            },
        );
        self.send_to_member(
            &mono.id,
            &member,
            ask.message.clone(),
            ask.app_request_id.clone(),
            false,
            cx,
        );
        if ask.wait {
            self.wait_for_member(call, &mono.id, ask.app_request_id, name, cx);
            return;
        }
        self.finish_call(
            call,
            Ok(json!({"member": name, "submitted": true, "notifyOnComplete": true, "accepted": true})),
            cx,
        );
    }

    /// group.access: what the group may open, or the user asked in its chat.
    fn group_access(
        &mut self,
        call: &Call,
        mono: &Mono,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        let now = now_ms() as i64;
        let groups = native::group_monos(&self.roster.monos);
        let (group_id, name, by) = if mono.is_group() {
            (mono.id.clone(), mono.label(), mono.label())
        } else {
            // A member, from its chat, its seat or a habit run, asks through
            // its group; in a seat, that seat's group.
            let seat = self.seat_owner(call.session).map(|(group, _)| group);
            let candidates: Vec<&GroupMono> = match &seat {
                Some(seat) => groups.iter().filter(|group| &group.id == seat).collect(),
                None => groups_of(&groups, &mono.id),
            };
            let target = group_app::access_group(&candidates, true, input)?;
            (target.id.clone(), target.name.clone(), mono.label())
        };
        let group = self
            .roster
            .get(&group_id)
            .and_then(|m| m.group.clone())
            .ok_or("Only a group or one of its members can ask for access")?;
        let reply = group_app::group_access(&group, &name, &by, input, &mono::new_id(), now)?;
        if let Some(next) = reply.group {
            if let Some(entry) = self.roster.get_mut(&group_id) {
                entry.group = Some(next);
            }
            self.save(cx);
            self.configure_chat(&group_id, cx);
        }
        Ok(reply.reply)
    }

    /// The user's tap on an access request in a group's chat: saved on the
    /// group so nobody asks again, and said in the chat so the group carries on.
    pub(super) fn answer_access(
        &mut self,
        group_id: &str,
        request: &str,
        access: Access,
        cx: &mut Context<Self>,
    ) {
        let Some(group) = self.roster.get(group_id).and_then(|m| m.group.clone()) else {
            return;
        };
        let Some(target) = group
            .requests
            .iter()
            .find(|entry| entry.id == request)
            .map(|entry| entry.target.clone())
        else {
            return;
        };
        let next = answer_access_request(&group, request, access, now_ms() as i64);
        if let Some(entry) = self.roster.get_mut(group_id) {
            entry.group = Some(next);
        }
        self.save(cx);
        self.configure_chat(group_id, cx);
        let text = access_answer_text(&target, access);
        if !self.group_submitted(group_id, &text, &[], cx) {
            self.enqueue(group_id, text, Vec::new(), false, cx);
        }
    }

    /// Who memory actions from this session write for: in a group's seat,
    /// the group, unless the fact is the member's own.
    pub(super) fn memory_owner(&self, session: i64, mono: &Mono, input: &Value) -> String {
        let seat = self.seat_owner(session).map(|(group, _)| group);
        group_app::memory_owner(
            &mono.id,
            seat.as_deref(),
            input.get("personal") == Some(&Value::Bool(true)),
        )
        .to_owned()
    }
}
