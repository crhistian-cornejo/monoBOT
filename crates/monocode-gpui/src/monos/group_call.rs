//! A group's voice call: dictation, member seats and interruptible local audio.
use super::*;
use crate::dictation::Dictation;
use gpui::{Div, FontWeight, Stateful, Task};
use monobot_voice::{
    Voice,
    queue::{Event as VoiceEvent, Queue as VoiceQueue},
};
use monocode_core::{
    dictation::Phase as MicPhase,
    mono::groups::{
        brief::{Member, group_members},
        call::{self, CallLine, Language, LineStatus},
        native,
        round::{SeatView, round_answer},
    },
    time::now_ms,
};
use std::{
    collections::VecDeque,
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
};

static LOCAL_VOICE: OnceLock<Arc<Voice>> = OnceLock::new();
static VOICE_INSTALLING: AtomicBool = AtomicBool::new(false);
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
/// How long the voices stay loaded after the last call.
const VOICE_IDLE: Duration = Duration::from_secs(120);
static VOICE_USES: VoiceUses = VoiceUses::new();

/// Which calls use the voices, so the release a hang-up schedules never
/// unloads Supertonic between the sentences of a live call, in any window.
pub(super) struct VoiceUses {
    /// Bumped by every start and hang-up: a release is skipped once another
    /// call started after it was scheduled.
    uses: AtomicU64,
    /// Calls holding the voices right now.
    holds: AtomicUsize,
}

impl VoiceUses {
    pub(super) const fn new() -> Self {
        Self {
            uses: AtomicU64::new(0),
            holds: AtomicUsize::new(0),
        }
    }

    /// A call starts using the voices, until the hold drops.
    pub(super) fn hold(&'static self) -> VoiceHold {
        self.holds.fetch_add(1, Ordering::AcqRel);
        self.uses.fetch_add(1, Ordering::AcqRel);
        VoiceHold(self)
    }

    /// A call hung up: the ticket its release checks when it runs.
    pub(super) fn hung_up(&self) -> u64 {
        self.uses.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Whether the release scheduled at `ticket` may run: no call holds the
    /// voices and none started since.
    pub(super) fn idle_since(&self, ticket: u64) -> bool {
        self.holds.load(Ordering::Acquire) == 0 && self.uses.load(Ordering::Acquire) == ticket
    }
}

/// Held by a call from the moment it loads the voices until it hangs up.
pub(super) struct VoiceHold(&'static VoiceUses);

impl Drop for VoiceHold {
    fn drop(&mut self) {
        self.0.holds.fetch_sub(1, Ordering::AcqRel);
    }
}

fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

fn local_voice() -> Arc<Voice> {
    LOCAL_VOICE
        .get_or_init(|| {
            Arc::new(Voice::new(
                monocode_core::store::monocode_data_dir()
                    .join("models")
                    .join("voice"),
            ))
        })
        .clone()
}

/// The call voice as Settings › Voice manages it: the same model, the same
/// download guard and the same holds the calls use.
pub(crate) mod model {
    use super::*;

    pub(crate) fn installed() -> bool {
        local_voice().installed()
    }

    pub(crate) fn installing() -> bool {
        VOICE_INSTALLING.load(Ordering::Acquire)
    }

    pub(crate) fn disk_bytes() -> u64 {
        local_voice().disk_bytes()
    }

    /// Downloads the voice unless a call or Settings already is. Blocks; run
    /// it off the interface thread.
    pub(crate) fn install(progress: impl FnMut(u64, u64)) -> Result<(), String> {
        if VOICE_INSTALLING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("The voices are already downloading.".into());
        }
        let _installing = InstallingVoice;
        local_voice()
            .install(progress)
            .map_err(|error| error.to_string())
    }

    /// Deletes the voice, never under a live call.
    pub(crate) fn remove() -> Result<(), String> {
        remove_from(&VOICE_USES, &local_voice())
    }

    pub(in crate::monos) fn remove_from(uses: &VoiceUses, voice: &Voice) -> Result<(), String> {
        if uses.holds.load(Ordering::Acquire) > 0 || installing() {
            return Err("Hang up the call before deleting its voice.".into());
        }
        voice.remove().map_err(|error| error.to_string())
    }
}

struct InstallingVoice;
impl Drop for InstallingVoice {
    fn drop(&mut self) {
        VOICE_INSTALLING.store(false, Ordering::Release);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    NeedsVoice,
    InstallingVoice,
    Starting,
    Listening,
    Answering,
    Draining,
}

impl Phase {
    fn live(self) -> bool {
        matches!(self, Self::Listening | Self::Answering | Self::Draining)
    }
}

struct Line {
    value: CallLine,
    note: Option<String>,
}

struct Stream {
    member: String,
    name: String,
    request: String,
    line: usize,
    offset: usize,
    spoken: Vec<String>,
    language: Option<Language>,
    voice: i32,
}

pub(super) struct CallState {
    group: String,
    phase: Phase,
    input: Entity<Input>,
    dictated: Entity<Input>,
    mic: Entity<Dictation>,
    _subscriptions: Vec<Subscription>,
    voice: Arc<Voice>,
    voice_hold: Option<VoiceHold>,
    queue: Option<VoiceQueue>,
    events: Option<Task<()>>,
    error: Option<String>,
    progress: Option<(u64, u64)>,
    epoch: u64,
    round: u64,
    started_at: u64,
    muted: bool,
    transcribing: bool,
    pause_epoch: u64,
    pause_text: Option<String>,
    previous_first: Option<String>,
    speakers: usize,
    voices: HashMap<String, usize>,
    members: Vec<Member>,
    lines: Vec<Line>,
    said: String,
    fallback: Language,
    addressed: bool,
    remaining: VecDeque<String>,
    earlier: Vec<(String, String)>,
    stream: Option<Stream>,
    speaking: Option<String>,
    /// Set while the call talks through a live model instead of this Mac.
    live: Option<live::LiveCall>,
    /// The voice goes into the user's ears, so the mic can stay open while
    /// the members answer and the user can cut in.
    in_ears: bool,
}

impl CallState {
    fn live(&self) -> bool {
        self.phase.live()
    }

    /// The mic stays open while members answer: only with the voice in the
    /// user's ears, so it never hears them, and never in a live call.
    fn overhears(&self) -> bool {
        self.in_ears
            && self.live.is_none()
            && matches!(self.phase, Phase::Answering | Phase::Draining)
    }

    /// What the members said this round, to tell their echo from the user.
    fn said_lately(&self) -> String {
        self.lines
            .iter()
            .rev()
            .take_while(|line| line.value.who != "user")
            .map(|line| line.value.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn call_button(id: &'static str, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .role(Role::Button)
        .aria_label(label.to_owned())
        .h(px(28.))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(content(0.12))
        .text_size(px(12.))
        .text_color(content(0.8))
        .cursor_pointer()
        .hover(|style| style.bg(content(0.06)))
        .child(label.to_owned())
}

impl Monos {
    fn call_members(&self, group: &Mono) -> Vec<Member> {
        let Some(group) = group.group.as_ref() else {
            return Vec::new();
        };
        let roster = native::members(&self.roster.monos);
        group_members(group, &roster).into_iter().cloned().collect()
    }

    fn ensure_call(&mut self, group: &Mono, cx: &mut Context<Self>) {
        if self
            .call
            .as_ref()
            .is_some_and(|call| call.group == group.id)
        {
            return;
        }
        self.call_hang_up(cx);
        let input = cx.new(|cx| Input::field("Say something", "Say something", content(0.35), cx));
        let dictated = cx.new(Input::session_composer);
        let pace = self.prefs.voice_pace.clone();
        let mic = cx.new(|cx| Dictation::for_call(&dictated, &pace, cx));
        let id = group.id.clone();
        let mic_events = cx.observe(&mic, move |this, _, cx| this.call_mic_changed(&id, cx));
        let id = group.id.clone();
        let submitted = cx.subscribe(&input, move |this, _, _: &input::Submitted, cx| {
            this.call_submit_text(&id, cx);
        });
        self.call = Some(CallState {
            group: group.id.clone(),
            phase: Phase::Idle,
            input,
            dictated,
            mic,
            _subscriptions: vec![mic_events, submitted],
            voice: local_voice(),
            voice_hold: None,
            queue: None,
            events: None,
            error: None,
            progress: None,
            epoch: next_generation(),
            round: 0,
            started_at: 0,
            muted: false,
            transcribing: false,
            pause_epoch: next_generation(),
            pause_text: None,
            previous_first: None,
            speakers: 1,
            voices: HashMap::new(),
            members: self.call_members(group),
            lines: Vec::new(),
            said: String::new(),
            fallback: Language::English,
            addressed: false,
            remaining: VecDeque::new(),
            earlier: Vec::new(),
            stream: None,
            speaking: None,
            live: None,
            in_ears: false,
        });
        let group = group.id.clone();
        let ears = cx
            .background_executor()
            .spawn(async move { monobot_dictation::devices::in_ears() });
        cx.spawn(async move |this, cx| {
            let in_ears = ears.await;
            let _ = this.update(cx, |this, _| {
                if let Some(call) = this.call.as_mut().filter(|call| call.group == group) {
                    call.in_ears = in_ears;
                }
            });
        })
        .detach();
    }

    fn call_start(&mut self, cx: &mut Context<Self>) {
        let choice = self.call_live_choice();
        let Some(call) = self.call.as_mut() else {
            return;
        };
        if call.phase != Phase::Idle || call.members.is_empty() {
            return;
        }
        call.error = None;
        call.lines.clear();
        if let Some(provider) = choice {
            self.call_start_live(provider, cx);
            return;
        }
        self.call_start_local(cx);
    }

    /// Starts the call with this Mac's voices.
    fn call_start_local(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        if !call.voice.installed() {
            call.phase = Phase::NeedsVoice;
            cx.notify();
            return;
        }
        self.call_prepare_voice(cx);
    }

    fn call_prepare_voice(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.epoch = next_generation();
        let (group, epoch, voice) = (call.group.clone(), call.epoch, call.voice.clone());
        call.phase = Phase::Starting;
        // Also cancels a release an earlier hang-up left waiting.
        call.voice_hold.get_or_insert_with(|| VOICE_USES.hold());
        let work = cx
            .background_executor()
            .spawn(async move { voice.speakers().map_err(|error| error.to_string()) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.call_voice_ready(&group, epoch, result, cx)
            });
        })
        .detach();
        cx.notify();
    }

    fn call_voice_ready(
        &mut self,
        group: &str,
        epoch: u64,
        result: Result<i32, String>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call.as_mut().filter(|call| {
            call.group == group && call.epoch == epoch && call.phase == Phase::Starting
        }) else {
            return;
        };
        let speakers = match result {
            Ok(speakers) => speakers.max(1) as usize,
            Err(error) => {
                call.error = Some(error);
                call.phase = Phase::Idle;
                cx.notify();
                return;
            }
        };
        let (events, incoming) = async_channel::unbounded();
        let queue = match VoiceQueue::new(call.voice.clone(), move |event| {
            let _ = events.try_send(event);
        }) {
            Ok(queue) => queue,
            Err(error) => {
                call.error = Some(error.to_string());
                call.phase = Phase::Idle;
                cx.notify();
                return;
            }
        };
        self.call_attach_queue(group, epoch, speakers, queue, incoming, cx);
    }

    fn call_attach_queue(
        &mut self,
        group: &str,
        epoch: u64,
        speakers: usize,
        queue: VoiceQueue,
        incoming: async_channel::Receiver<VoiceEvent>,
        cx: &mut Context<Self>,
    ) {
        let Some(call) = self.call.as_mut().filter(|call| {
            call.group == group && call.epoch == epoch && call.phase == Phase::Starting
        }) else {
            return;
        };
        let id = group.to_owned();
        call.events = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = incoming.recv().await {
                if this
                    .update(cx, |this, cx| this.call_voice_event(&id, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        }));
        call.queue = Some(queue);
        call.speakers = speakers;
        call.started_at = now_ms();
        call.phase = Phase::Listening;
        self.call_listen(cx);
        cx.notify();
    }

    fn call_install_voice(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.phase == Phase::NeedsVoice)
        else {
            return;
        };
        if call.voice.installed() {
            self.call_prepare_voice(cx);
            return;
        }
        if VOICE_INSTALLING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            call.error =
                Some("The voices are already downloading. Try again when they finish.".into());
            cx.notify();
            return;
        }
        call.epoch = next_generation();
        let (group, epoch, voice) = (call.group.clone(), call.epoch, call.voice.clone());
        call.phase = Phase::InstallingVoice;
        call.progress = Some((0, monobot_voice::model::ARCHIVE.bytes));
        let (report, incoming) = async_channel::unbounded();
        cx.background_executor()
            .spawn(async move {
                let _installing = InstallingVoice;
                let mut last = 0;
                let result = voice.install(|done, total| {
                    if done == total || done.saturating_sub(last) >= 1 << 20 {
                        last = done;
                        let _ = report.try_send(Ok((done, total)));
                    }
                });
                let _ = report.try_send(Err(result.map_err(|error| error.to_string())));
            })
            .detach();
        cx.spawn(async move |this, cx| {
            while let Ok(update) = incoming.recv().await {
                let done = update.is_err();
                let alive = this.update(cx, |this, cx| {
                    let Some(call) = this.call.as_mut().filter(|call| {
                        call.group == group
                            && call.epoch == epoch
                            && call.phase == Phase::InstallingVoice
                    }) else {
                        return;
                    };
                    match update {
                        Ok(progress) => call.progress = Some(progress),
                        Err(Ok(())) => this.call_prepare_voice(cx),
                        Err(Err(error)) => {
                            call.error = Some(error);
                            call.phase = Phase::NeedsVoice;
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() || done {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn call_hang_up(&mut self, cx: &mut Context<Self>) {
        let Some(mut call) = self.call.take() else {
            return;
        };
        let live = call.live();
        let voice = call.voice.clone();
        if let Some(queue) = call.queue.as_mut() {
            queue.interrupt();
        }
        call.mic.update(cx, |mic, cx| mic.cancel_pending(cx));
        if let Some(stream) = call.stream.take() {
            self.cancel_call_seat(&call.group, &stream.member, &stream.request, cx);
        }
        if let Some(live_call) = call.live.take() {
            self.call_live_end(live_call, &call.group.clone(), cx);
        }
        let live = live || call.lines.iter().any(|line| line.value.who == "live");
        if live {
            let lines: Vec<_> = call.lines.into_iter().map(|line| line.value).collect();
            let date = chrono::DateTime::from_timestamp_millis(call.started_at as i64)
                .map(|at| {
                    at.with_timezone(&chrono::Local)
                        .format("%b %e, %Y, %H:%M")
                        .to_string()
                })
                .unwrap_or_default();
            if let Some(transcript) = call::call_transcript(
                &self
                    .roster
                    .get(&call.group)
                    .map_or_else(|| "Group".into(), Mono::label),
                &date,
                &lines,
            ) {
                let now = now_ms();
                if let Some(group) = self.roster.get_mut(&call.group) {
                    group.documents.push(mono::Document {
                        id: mono::new_id(),
                        kind: "document".into(),
                        title: transcript.title,
                        summary: String::new(),
                        body: transcript.body,
                        created_at: now,
                        updated_at: now,
                    });
                    self.save(cx);
                }
            }
        }
        drop(call.voice_hold.take());
        let ticket = VOICE_USES.hung_up();
        cx.spawn(async move |_, cx| {
            cx.background_executor().timer(VOICE_IDLE).await;
            if !VOICE_USES.idle_since(ticket) {
                return;
            }
            cx.background_executor()
                .spawn(async move {
                    voice.release();
                })
                .detach();
        })
        .detach();
        cx.notify();
    }

    fn call_listen(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref() else {
            return;
        };
        if call.muted || !(call.phase == Phase::Listening || call.overhears()) {
            return;
        }
        let mic = call.mic.clone();
        let ready = {
            let mic = mic.read(cx);
            mic.phase() == MicPhase::Idle && mic.error().is_none()
        };
        if ready {
            mic.update(cx, |mic, cx| mic.toggle(cx));
        }
    }

    fn call_mic_changed(&mut self, group: &str, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.group == group && call.live())
        else {
            return;
        };
        if call.muted || call.phase != Phase::Listening {
            let mic = call.mic.clone();
            if !call.muted && call.overhears() {
                // The user cuts in: real words, not a backchannel and not
                // the members' own voice heard back.
                let ended = mic.update(cx, |mic, _| mic.take_turn_end());
                let heard = mic.read(cx).partial().trim().to_owned();
                if ended
                    && monobot_dictation::turn::cuts_in(&heard)
                    && !monobot_dictation::turn::echo_of(&heard, &call.said_lately())
                {
                    self.call_say(heard, cx);
                } else if ended {
                    // A backchannel: listen on for the next turn.
                    mic.update(cx, |mic, cx| mic.cancel(cx));
                } else if mic.read(cx).phase() == MicPhase::Idle {
                    self.call_listen(cx);
                }
                return;
            }
            if mic.read(cx).phase().cancellable() {
                mic.update(cx, |mic, cx| mic.cancel(cx));
            }
            return;
        }
        let (phase, speaking, partial, decides) = {
            let mic = call.mic.read(cx);
            (
                mic.phase(),
                mic.speaking(),
                mic.partial().to_owned(),
                mic.decides_turns(),
            )
        };
        // The session ended the turn (its pause, its words, Smart Turn).
        let mic = call.mic.clone();
        if decides && mic.update(cx, |mic, _| mic.take_turn_end()) {
            if phase == MicPhase::Listening && !partial.trim().is_empty() {
                // What the mic hands back when it stops is this turn.
                if let Some(call) = self.call.as_mut() {
                    call.transcribing = true;
                }
                mic.update(cx, |mic, cx| mic.toggle(cx));
            }
            cx.notify();
            return;
        }
        if phase == MicPhase::Finishing {
            call.transcribing = true;
        }
        if phase == MicPhase::Idle && call.transcribing {
            call.transcribing = false;
            let text = call.dictated.read(cx).content.trim().to_owned();
            call.dictated
                .update(cx, |input, cx| input.set_text(String::new(), cx));
            if text.is_empty() {
                self.call_listen(cx);
            } else {
                self.call_say(text, cx);
            }
            cx.notify();
            return;
        }
        if phase == MicPhase::Idle {
            self.call_listen(cx);
            return;
        }
        // Without a session that decides, a fixed pause ends the turn.
        let quiet =
            !decides && phase == MicPhase::Listening && !speaking && !partial.trim().is_empty();
        if quiet && call.pause_text.as_deref() != Some(&partial) {
            call.pause_epoch = next_generation();
            call.pause_text = Some(partial);
            let epoch = call.pause_epoch;
            let group = group.to_owned();
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(call::TURN_GAP_MS))
                    .await;
                let _ = this.update(cx, |this, cx| this.call_end_silence(&group, epoch, cx));
            })
            .detach();
        } else if !quiet && call.pause_text.take().is_some() {
            call.pause_epoch = next_generation();
        }
        cx.notify();
    }

    fn call_end_silence(&mut self, group: &str, epoch: u64, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_ref().filter(|call| {
            call.group == group && call.phase == Phase::Listening && call.pause_epoch == epoch
        }) else {
            return;
        };
        let mic = call.mic.clone();
        let quiet = {
            let mic = mic.read(cx);
            mic.phase() == MicPhase::Listening
                && !mic.speaking()
                && !mic.partial().trim().is_empty()
        };
        if quiet {
            mic.update(cx, |mic, cx| mic.toggle(cx));
        }
    }

    fn call_submit_text(&mut self, group: &str, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_ref()
            .filter(|call| call.group == group && call.live())
        else {
            return;
        };
        let text = call.input.read(cx).content.trim().to_owned();
        if !text.is_empty() {
            call.input
                .update(cx, |input, cx| input.set_text(String::new(), cx));
            self.call_say(text, cx);
        }
    }

    fn call_voice_event(&mut self, group: &str, event: VoiceEvent, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_mut()
            .filter(|call| call.group == group && call.live())
        else {
            return;
        };
        match event {
            VoiceEvent::Speaking { round, member } if round == call.round => {
                call.speaking = Some(member);
            }
            VoiceEvent::Finished {
                round,
                member,
                error,
            } if round == call.round => {
                if call.speaking.as_deref() == Some(&member) {
                    call.speaking = None;
                }
                if let Some(error) = error {
                    call.error = Some(error);
                }
            }
            VoiceEvent::Drained { round }
                if round == call.round && call.phase == Phase::Draining =>
            {
                call.speaking = None;
                call.phase = Phase::Listening;
                self.call_listen(cx);
            }
            _ => {}
        }
        cx.notify();
    }

    fn call_abort_round(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut() else {
            return;
        };
        if let Some(queue) = call.queue.as_mut() {
            queue.interrupt();
        }
        call.pause_epoch = next_generation();
        call.pause_text = None;
        call.transcribing = false;
        call.speaking = None;
        call.remaining.clear();
        for line in &mut call.lines {
            if line.value.status == LineStatus::Thinking {
                line.value.status = LineStatus::Stopped;
            }
        }
        let target = call
            .stream
            .take()
            .map(|stream| (call.group.clone(), stream.member, stream.request));
        call.dictated
            .update(cx, |input, cx| input.set_text(String::new(), cx));
        let mic = call.mic.clone();
        mic.update(cx, |mic, cx| mic.cancel_pending(cx));
        if let Some((group, member, request)) = target {
            self.cancel_call_seat(&group, &member, &request, cx);
        }
    }

    fn call_say(&mut self, text: String, cx: &mut Context<Self>) {
        let text = text.trim().to_owned();
        let Some(group_id) = self
            .call
            .as_ref()
            .filter(|call| call.live())
            .map(|call| call.group.clone())
        else {
            return;
        };
        if text.is_empty() {
            return;
        }
        let Some(group) = self.roster.get(&group_id).cloned() else {
            self.call_hang_up(cx);
            return;
        };
        let members = self.call_members(&group);
        self.call_abort_round(cx);
        let Some(call) = self.call.as_mut() else {
            return;
        };
        call.error = None;
        call.members = members;
        // «Hola yares…» calls Jarvis as if his name had been heard right.
        let text = call::spoken_address(&call.members, &text).unwrap_or(text);
        let order = call::turn_order(&call.members, &text, call.previous_first.as_deref());
        if !order.addressed {
            call.previous_first = order.order.first().cloned();
        }
        let ids: Vec<_> = call
            .members
            .iter()
            .map(|member| member.id.clone())
            .collect();
        call.voices = call::member_voices(&ids, call.speakers);
        let locale = sys_locale::get_locale().unwrap_or_else(|| "en".into());
        call.fallback = call::reply_language(&text, call::default_language(&locale));
        call.addressed = order.addressed;
        call.remaining = order.order.into();
        call.earlier.clear();
        call.said = text.clone();
        call.lines.push(Line {
            value: CallLine {
                who: "user".into(),
                name: "You".into(),
                text,
                status: LineStatus::Said,
            },
            note: None,
        });
        let Some(queue) = call.queue.as_mut() else {
            call.error = Some("The voice queue is not available".into());
            call.phase = Phase::Listening;
            self.call_listen(cx);
            cx.notify();
            return;
        };
        call.round = queue.begin_round();
        call.phase = Phase::Answering;
        self.call_ask_next(cx);
        cx.notify();
    }

    fn call_ask_next(&mut self, cx: &mut Context<Self>) {
        loop {
            let Some(group_id) = self
                .call
                .as_ref()
                .filter(|call| call.phase == Phase::Answering)
                .map(|call| call.group.clone())
            else {
                return;
            };
            let Some(group) = self.roster.get(&group_id).cloned() else {
                self.call_hang_up(cx);
                return;
            };
            let Some(call) = self.call.as_mut() else {
                return;
            };
            if call.stream.is_some() {
                return;
            }
            if !call.addressed && call.earlier.len() >= call::MAX_SPEAKERS {
                call.remaining.clear();
            }
            let member = loop {
                match call.remaining.pop_front() {
                    Some(id) => {
                        if let Some(member) = call.members.iter().find(|member| member.id == id) {
                            break Some(member.clone());
                        }
                    }
                    None => break None,
                }
            };
            let Some(member) = member else {
                call.phase = Phase::Draining;
                let result = call.queue.as_ref().map_or_else(
                    || Err("The voice queue is not available".into()),
                    |queue| queue.drain(call.round),
                );
                if let Err(error) = result {
                    call.error = Some(error);
                    call.phase = Phase::Listening;
                    self.call_listen(cx);
                }
                cx.notify();
                return;
            };
            let others: Vec<String> = call
                .members
                .iter()
                .filter(|other| other.id != member.id)
                .map(|other| other.name.clone())
                .collect();
            let prompt = call::call_message(
                &group.label(),
                group.group.as_ref().and_then(|group| group.goal.as_deref()),
                &others,
                &call.said,
                &call.earlier,
                call.addressed,
            );
            let request = format!("call-{}-{}-{}", group_id, call.round, mono::new_id());
            let line = call.lines.len();
            call.lines.push(Line {
                value: CallLine {
                    who: member.id.clone(),
                    name: member.name.clone(),
                    text: String::new(),
                    status: LineStatus::Thinking,
                },
                note: None,
            });
            call.stream = Some(Stream {
                member: member.id.clone(),
                name: member.name,
                request: request.clone(),
                line,
                offset: 0,
                spoken: Vec::new(),
                language: None,
                voice: *call.voices.get(&member.id).unwrap_or(&0) as i32,
            });
            if let Err(error) = self.call_send_member(&group_id, &member.id, prompt, request, cx) {
                if let Some(call) = self.call.as_mut()
                    && let Some(stream) = call.stream.take()
                {
                    call.lines[stream.line].value.status = LineStatus::Busy;
                    call.lines[stream.line].note = Some(error);
                }
                continue;
            }
            cx.notify();
            return;
        }
    }

    /// Follow the current seat's streamed rows, without involving the lead's chat.
    pub(super) fn call_progress(&mut self, cx: &mut Context<Self>) {
        let Some((group, member, request)) = self.call.as_ref().and_then(|call| {
            (call.phase == Phase::Answering)
                .then_some(call.stream.as_ref()?)
                .map(|stream| {
                    (
                        call.group.clone(),
                        stream.member.clone(),
                        stream.request.clone(),
                    )
                })
        }) else {
            return;
        };
        let failed = self.call_request_failed(&request);
        let seat = self
            .groups
            .seats
            .get(&(group.clone(), member))
            .and_then(|seat| seat.view.as_ref())
            .map(|view| {
                let view = view.read(cx);
                (native::blocks(view.mono_rows(cx)), view.busy)
            });
        let (reply, done, error) = match seat {
            Some((blocks, busy)) => {
                let outcome = round_answer(
                    SeatView {
                        blocks: &blocks,
                        busy,
                    },
                    &request,
                );
                let error = outcome.as_ref().and_then(|answer| answer.error.clone());
                (
                    call::member_reply(&blocks, &request),
                    failed || outcome.is_some(),
                    error,
                )
            }
            None => (String::new(), failed, None),
        };
        let Some(call) = self.call.as_mut() else {
            return;
        };
        let Some(stream) = call
            .stream
            .as_mut()
            .filter(|stream| stream.request == request)
        else {
            return;
        };
        if stream.offset == 0 && !done && call::could_be_pass(&reply) {
            return;
        }
        if stream.offset == 0 && done && call::is_pass(&reply) {
            call.lines[stream.line].value.status = LineStatus::Passed;
        } else {
            let taken = call::take_sentences(&reply, stream.offset, done);
            stream.offset = taken.next;
            for sentence in taken.sentences {
                if stream.spoken.is_empty() && call::is_pass(&sentence) {
                    continue;
                }
                let language = *stream
                    .language
                    .get_or_insert_with(|| call::reply_language(&reply, call.fallback));
                if let Some(queue) = call.queue.as_ref()
                    && let Err(error) = queue.say(
                        call.round,
                        &stream.member,
                        &sentence,
                        language.as_str(),
                        stream.voice,
                    )
                {
                    call.error = Some(error);
                }
                stream.spoken.push(sentence);
                call.lines[stream.line].value.text = stream.spoken.join(" ");
                call.lines[stream.line].value.status = LineStatus::Said;
            }
        }
        if done {
            let stream = call.stream.take().unwrap();
            if stream.spoken.is_empty() {
                let line = &mut call.lines[stream.line];
                if failed || error.is_some() {
                    line.value.status = LineStatus::Failed;
                    line.note = error.or_else(|| Some("The member could not answer".into()));
                } else {
                    line.value.status = LineStatus::Passed;
                }
            } else {
                call.earlier.push((stream.name, stream.spoken.join(" ")));
            }
            self.call_ask_next(cx);
        }
        cx.notify();
    }

    pub(super) fn call_seat_changed(&mut self, key: &(String, String), cx: &mut Context<Self>) {
        let current = self.call.as_ref().is_some_and(|call| {
            call.group == key.0
                && call
                    .stream
                    .as_ref()
                    .is_some_and(|stream| stream.member == key.1)
        });
        if current {
            self.call_progress(cx);
        }
        if self.call_live_asking(&key.1) {
            self.call_live_progress(&key.1, cx);
        }
    }

    fn call_toggle_mute(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self.call.as_mut().filter(|call| call.live()) else {
            return;
        };
        call.muted = !call.muted;
        if call.muted {
            call.transcribing = false;
            call.dictated
                .update(cx, |input, cx| input.set_text(String::new(), cx));
            call.mic.update(cx, |mic, cx| mic.cancel_pending(cx));
        } else {
            self.call_listen(cx);
        }
        cx.notify();
    }

    fn call_interrupt(&mut self, cx: &mut Context<Self>) {
        if !self.call.as_ref().is_some_and(CallState::live) {
            return;
        }
        self.call_abort_round(cx);
        if let Some(call) = self.call.as_mut() {
            call.phase = Phase::Listening;
        }
        self.call_listen(cx);
        cx.notify();
    }

    fn call_end_turn(&mut self, cx: &mut Context<Self>) {
        let Some(call) = self
            .call
            .as_ref()
            .filter(|call| call.phase == Phase::Listening)
        else {
            return;
        };
        let mic = call.mic.clone();
        if mic.read(cx).phase() == MicPhase::Listening {
            mic.update(cx, |mic, cx| mic.toggle(cx));
        }
    }

    pub(super) fn call_panel(&mut self, group: &Mono, cx: &mut Context<Self>) -> AnyElement {
        // Looking at another group keeps a live call going: only hanging up
        // ends it.
        if let Some(other) = self
            .call
            .as_ref()
            .filter(|call| call.live() && call.group != group.id)
        {
            let name = self
                .roster
                .get(&other.group)
                .map_or_else(|| "another group".into(), Mono::label);
            return div()
                .debug_selector(|| "group-call-elsewhere".into())
                .p_3()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .px_2()
                        .text_size(px(13.))
                        .text_color(content(0.7))
                        .child(format!(
                            "You are in a call with {name}. Hang up to call this group."
                        )),
                )
                .child(
                    div().px_2().flex().child(
                        call_button("group-call-hang-up-elsewhere", "Hang up")
                            .on_click(cx.listener(|this, _, _, cx| this.call_hang_up(cx))),
                    ),
                )
                .into_any_element();
        }
        self.ensure_call(group, cx);
        let members = self.call_members(group);
        if let Some(call) = self.call.as_mut().filter(|call| !call.live()) {
            call.members = members;
        }
        let Some(call) = self.call.as_ref() else {
            return div().into_any_element();
        };
        let live = call.live();
        let status = match call.phase {
            Phase::Idle | Phase::NeedsVoice => {
                "Talk with the group out loud. Name someone to hear just them.".to_owned()
            }
            Phase::InstallingVoice => "Downloading the voices...".to_owned(),
            Phase::Starting => "Getting the voices ready...".to_owned(),
            Phase::Listening if call.muted => "Muted. Unmute to talk, or type below.".to_owned(),
            Phase::Listening => {
                "Your turn. Talk, then pause; the members answer when you stop.".to_owned()
            }
            Phase::Answering | Phase::Draining => {
                if let Some(name) = call
                    .speaking
                    .as_ref()
                    .and_then(|id| call.members.iter().find(|member| &member.id == id))
                    .map(|member| member.name.as_str())
                {
                    format!("{name} is speaking.")
                } else if let Some(stream) = &call.stream {
                    format!("{} is thinking...", stream.name)
                } else {
                    "The members are answering...".to_owned()
                }
            }
        };
        let member_rows: Vec<AnyElement> =
            call.members
                .iter()
                .map(|member| {
                    let speaking = call.speaking.as_deref() == Some(&member.id);
                    let thinking = call
                        .stream
                        .as_ref()
                        .is_some_and(|stream| stream.member == member.id);
                    let look = self.roster.get(&member.id);
                    let mascot = look.map_or("robot", |mono| mono.mascot.as_str());
                    let color = look.map_or(mono::COLORS_MONO[0], |mono| mono.color);
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .when(speaking, |row| row.bg(accent_color().opacity(0.1)))
                        .child(avatar::avatar(
                            mascot,
                            color,
                            if speaking || thinking {
                                "working"
                            } else {
                                "idle"
                            },
                            26.,
                            false,
                        ))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(FontWeight::MEDIUM)
                                .child(member.name.clone()),
                        )
                        .child(div().text_size(px(11.)).text_color(content(0.5)).child(
                            if speaking {
                                "Speaking"
                            } else if thinking {
                                "Thinking..."
                            } else {
                                ""
                            },
                        ))
                        .into_any_element()
                })
                .collect();
        let transcript: Vec<AnyElement> = call
            .lines
            .iter()
            .map(|line| {
                let status = match line.value.status {
                    LineStatus::Said => "",
                    LineStatus::Thinking => "Thinking...",
                    LineStatus::Passed => "Had nothing to add.",
                    LineStatus::Busy => "Busy right now.",
                    LineStatus::Failed => "Could not answer.",
                    LineStatus::Stopped => "Stopped.",
                };
                let text = if line.value.text.is_empty() {
                    line.note.as_deref().unwrap_or(status).to_owned()
                } else {
                    line.value.text.clone()
                };
                div()
                    .flex()
                    .flex_wrap()
                    .gap_1()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(format!("{}:", line.value.name)),
                    )
                    .child(
                        div()
                            .text_color(content(if line.value.text.is_empty() {
                                0.45
                            } else {
                                0.8
                            }))
                            .child(text),
                    )
                    .into_any_element()
            })
            .collect();
        let error = call.error.clone();
        let phase = call.phase;
        let muted = call.muted;
        let empty = call.members.is_empty();
        let mic = call.mic.clone();
        let input = call.input.clone();
        let can_finish = {
            let mic = mic.read(cx);
            mic.phase() == MicPhase::Listening && !mic.partial().trim().is_empty()
        };
        let progress = call.progress;
        let content_area = div()
            .id("group-call-content")
            .debug_selector(|| "group-call-content".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(div().px_2().text_size(px(13.)).text_color(content(0.7)).child(status))
            .when(empty, |view| view.child(
                div().px_2().text_size(px(12.)).text_color(content(0.5))
                    .child("Add members on the Members tab to call them."),
            ))
            .when(!empty, |view| view.child(
                div().border_1().border_color(content(0.1)).flex().flex_col().children(member_rows),
            ))
            .when(!transcript.is_empty(), |view| view.child(
                div().px_2().flex().flex_col().gap_1p5().children(transcript),
            ))
            .when_some(error, |view, error| view.child(
                div().px_2().py_1().border_1().border_color(content(0.1))
                    .flex().items_center().gap_2()
                    .child(div().flex_1().text_size(px(12.)).text_color(gpui::rgb(crate::right_panel::RED_400)).child(error))
                    .child(call_button("group-call-dismiss", "Dismiss").on_click(cx.listener(|this, _, _, cx| {
                        if let Some(call) = this.call.as_mut() { call.error = None; }
                        cx.notify();
                    }))),
            ))
            .when(phase == Phase::NeedsVoice, |view| view.child(
                div().px_2().py_2().border_1().border_color(content(0.1))
                    .flex().items_center().gap_2()
                    .child(div().flex_1().text_size(px(12.)).text_color(content(0.7))
                            .child(format!(
                                "Download the local Spanish and English voices once ({}) to start.",
                                monocode_core::dictation::format_model_size(monobot_voice::model::ARCHIVE.bytes),
                            )))
                    .child(call_button("group-call-install", "Download").on_click(cx.listener(|this, _, _, cx| {
                        this.call_install_voice(cx);
                    }))),
            ))
            .when(phase == Phase::InstallingVoice, |view| view.child({
                let percent = progress.map_or(0, |(done, total)| done.saturating_mul(100).checked_div(total).unwrap_or(0));
                div().px_2().text_size(px(12.)).text_color(content(0.7))
                    .child(format!("Downloading voices... {percent}%"))
            }))
            .when(live, |view| view.child(mic.clone()));
        let controls = div()
            .px_3()
            .pb_2()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .when(live, |view| {
                let view = view.child(
                    call_button("group-call-mute", if muted { "Unmute" } else { "Mute" })
                        .on_click(cx.listener(|this, _, _, cx| this.call_toggle_mute(cx))),
                );
                let view = if phase == Phase::Listening {
                    view.child(
                        call_button("group-call-done", "Done talking")
                            .when(!can_finish, |button| button.opacity(0.4).cursor_default())
                            .when(can_finish, |button| {
                                button
                                    .on_click(cx.listener(|this, _, _, cx| this.call_end_turn(cx)))
                            }),
                    )
                } else {
                    view.child(
                        call_button("group-call-interrupt", "Interrupt")
                            .on_click(cx.listener(|this, _, _, cx| this.call_interrupt(cx))),
                    )
                };
                view.child(
                    call_button("group-call-hang-up", "Hang up")
                        .on_click(cx.listener(|this, _, _, cx| this.call_hang_up(cx))),
                )
            })
            .when(!live, |view| {
                let enabled = !empty && phase == Phase::Idle;
                view.child(
                    call_button("group-call-start", "Start call")
                        .when(!enabled, |button| button.opacity(0.4).cursor_default())
                        .when(enabled, |button| {
                            button
                                .bg(accent_color().opacity(0.15))
                                .on_click(cx.listener(|this, _, _, cx| this.call_start(cx)))
                        }),
                )
            });
        div()
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(content_area)
            .child(controls)
            .when(live, |view| {
                view.child(
                    div()
                        .px_3()
                        .pb_3()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(input))
                        .child(call_button("group-call-send", "Send").on_click(cx.listener(
                            |this, _, _, cx| {
                                if let Some(group) =
                                    this.call.as_ref().map(|call| call.group.clone())
                                {
                                    this.call_submit_text(&group, cx);
                                }
                            },
                        ))),
                )
            })
            .into_any_element()
    }

    #[cfg(test)]
    pub(super) fn call_test_start(
        &mut self,
        group: &Mono,
        backend: impl monobot_voice::queue::Backend,
        cx: &mut Context<Self>,
    ) {
        self.ensure_call(group, cx);
        let call = self.call.as_mut().unwrap();
        call.epoch = next_generation();
        call.phase = Phase::Starting;
        let epoch = call.epoch;
        let (events, incoming) = async_channel::unbounded();
        let queue = VoiceQueue::with_backend(backend, move |event| {
            let _ = events.try_send(event);
        })
        .unwrap();
        self.call_attach_queue(&group.id, epoch, 10, queue, incoming, cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_in_ears(&mut self, in_ears: bool) {
        if let Some(call) = self.call.as_mut() {
            call.in_ears = in_ears;
        }
    }

    #[cfg(test)]
    pub(super) fn call_test_answering(&self) -> bool {
        self.call
            .as_ref()
            .is_some_and(|call| matches!(call.phase, Phase::Answering | Phase::Draining))
    }

    #[cfg(test)]
    pub(super) fn call_test_say(&mut self, text: &str, cx: &mut Context<Self>) {
        self.call_say(text.into(), cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_interrupt(&mut self, cx: &mut Context<Self>) {
        self.call_interrupt(cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_mute(&mut self, cx: &mut Context<Self>) {
        self.call_toggle_mute(cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_mic(&self) -> Entity<Dictation> {
        self.call.as_ref().unwrap().mic.clone()
    }

    #[cfg(test)]
    pub(super) fn call_test_input(&self) -> Entity<Input> {
        self.call.as_ref().unwrap().input.clone()
    }

    #[cfg(test)]
    pub(super) fn call_test_dictated(&self) -> Entity<Input> {
        self.call.as_ref().unwrap().dictated.clone()
    }

    #[cfg(test)]
    pub(super) fn call_test_pending_voice(&mut self, group: &Mono, cx: &mut Context<Self>) -> u64 {
        self.ensure_call(group, cx);
        let call = self.call.as_mut().unwrap();
        call.epoch = next_generation();
        call.phase = Phase::Starting;
        call.epoch
    }

    #[cfg(test)]
    pub(super) fn call_test_voice_failure(
        &mut self,
        group: &str,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        self.call_voice_ready(group, epoch, Err("voice failure".into()), cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_voice_error(&self) -> Option<String> {
        self.call.as_ref()?.error.clone()
    }

    #[cfg(test)]
    pub(super) fn call_test_pause_epoch(&self) -> u64 {
        self.call.as_ref().unwrap().pause_epoch
    }

    #[cfg(test)]
    pub(super) fn call_test_end_silence(
        &mut self,
        group: &str,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        self.call_end_silence(group, epoch, cx);
    }

    #[cfg(test)]
    pub(super) fn call_test_current_request(&self) -> Option<(String, String)> {
        self.call
            .as_ref()?
            .stream
            .as_ref()
            .map(|stream| (stream.member.clone(), stream.request.clone()))
    }

    #[cfg(test)]
    pub(super) fn call_test_lines(&self) -> Vec<CallLine> {
        self.call.as_ref().map_or_else(Vec::new, |call| {
            call.lines.iter().map(|line| line.value.clone()).collect()
        })
    }
}

#[path = "group_call_live.rs"]
mod live;

#[cfg(test)]
pub(super) mod live_test {
    pub(in crate::monos) use super::live::Message;
    pub(in crate::monos) use monobot_live::session::State;
}
