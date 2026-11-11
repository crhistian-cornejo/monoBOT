use monocode_core::agent::{Approval, Control, Settings};
use monocode_core::{
    provider::{self, Event, Kind, Request},
    store::{Session, Store},
    transcript::{Row, Transcript},
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
pub(crate) mod contributions;
#[cfg(all(test, unix))]
mod follow_up_tests;
mod handoff;
#[cfg(all(test, unix))]
mod handoff_tests;
mod persistence;
pub(crate) mod remote;
mod visual_discovery;
#[cfg(all(test, unix))]
mod visual_tests;
use persistence::PendingSave;
pub(crate) type Database = Arc<Mutex<Store>>;
struct TurnInput {
    summary: Session,
    prompt: String,
    settings: Settings,
}
pub(crate) struct Update {
    pub(crate) worktree_cwd: Option<String>,
    pub(crate) commands: Option<Vec<provider::NativeCommand>>,
    pub(crate) provider_turn_id: Option<String>,
    pub(crate) turn_ready: bool,
    pub(crate) follow_up_delivered: Vec<String>,
    pub(crate) follow_up_errors: Vec<(String, String)>,
    pub(crate) delivered: Option<String>,
    pub(crate) start: usize,
    pub(crate) rows: Vec<Row>,
    pub(crate) provider_id: Option<String>,
    pub(crate) model: Option<String>,
    pub(crate) config_model: Option<String>,
    pub(crate) config_settings: Option<std::collections::HashMap<String, String>>,
    /// A title the host changed since the last update; never written back.
    pub(crate) title: Option<String>,
    pub(crate) context: Option<monocode_core::context::Usage>,
    pub(crate) metrics: Option<monocode_core::agent::TurnMetrics>,
    pub(crate) usage_limit: Option<monocode_core::usage::Limit>,
    /// The tasks a yielded turn still waits on, when that changed.
    pub(crate) background: Option<Vec<provider::BackgroundTask>>,
    pub(crate) approvals: Vec<Approval>,
    pub(crate) approvals_resolved: Vec<String>,
    pub(crate) questions: Vec<monocode_core::agent::Question>,
    pub(crate) questions_resolved: Vec<String>,
    pub(crate) handoff: Option<Option<monocode_core::agent::Handoff>>,
    pub(crate) status: String,
    pub(crate) finished: bool,
}
impl Update {
    /// Only a new status line, while the turn waits on something after its
    /// provider finished.
    fn status(start: usize, status: &str) -> Self {
        Self {
            worktree_cwd: None,
            commands: None,
            provider_turn_id: None,
            turn_ready: false,
            follow_up_delivered: Vec::new(),
            follow_up_errors: Vec::new(),
            delivered: None,
            start,
            rows: Vec::new(),
            provider_id: None,
            model: None,
            config_model: None,
            config_settings: None,
            title: None,
            context: None,
            metrics: None,
            usage_limit: None,
            background: None,
            approvals: Vec::new(),
            approvals_resolved: Vec::new(),
            questions: Vec::new(),
            questions_resolved: Vec::new(),
            handoff: None,
            status: status.into(),
            finished: false,
        }
    }
}
/// Appends a structured plan as the transcript's `plan` row.
fn plan_row(transcript: &mut Transcript, text: String) -> Result<usize, String> {
    let at = transcript.begin(monocode_core::plan::ROLE);
    let row = monocode_core::plan::PlanRow {
        text,
        answer: false,
    };
    transcript.append(&serde_json::to_string(&row).map_err(|e| e.to_string())?);
    Ok(at)
}
fn publish(
    tx: &async_channel::Sender<Update>,
    cancel: &AtomicBool,
    mut update: Update,
) -> Result<(), String> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        match tx.try_send(update) {
            Ok(()) => return Ok(()),
            Err(async_channel::TrySendError::Closed(_)) => return Err("Window closed".into()),
            Err(async_channel::TrySendError::Full(value)) => update = value,
        }
        std::thread::sleep(Duration::from_millis(4));
    }
}
pub(crate) fn locked(store: &Database) -> Result<std::sync::MutexGuard<'_, Store>, String> {
    store
        .lock()
        .map_err(|_| "Native session store unavailable".into())
}

/// The conversation's controls, joined by the loop guard's reminders. The
/// thread that forwards the controls ends with the turn.
struct Reminders {
    send: std::sync::mpsc::Sender<Control>,
    ended: Arc<AtomicBool>,
}

impl Reminders {
    fn join(
        controls: std::sync::mpsc::Receiver<Control>,
    ) -> (Self, std::sync::mpsc::Receiver<Control>) {
        let (send, joined) = std::sync::mpsc::channel();
        let ended = Arc::new(AtomicBool::new(false));
        let (forward, done) = (send.clone(), ended.clone());
        std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                match controls.recv_timeout(Duration::from_millis(100)) {
                    Ok(control) => {
                        if forward.send(control).is_err() {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        (Self { send, ended }, joined)
    }

    /// Steer the turn with a reminder, as if queued mid-turn.
    fn send(&self, n: usize, text: String) {
        let id = monocode_core::loop_guard::reminder_id(n);
        let _ = self.send.send(Control {
            id: id.clone(),
            decision: monocode_core::agent::Decision::Steer(monocode_core::follow_up::Message {
                id,
                text,
                steer: true,
                ..Default::default()
            }),
        });
    }
}

impl Drop for Reminders {
    fn drop(&mut self) {
        self.ended.store(true, Ordering::Relaxed);
    }
}

pub(crate) fn run_turn(
    summary: Session,
    prompt: String,
    settings: Settings,
    controls: std::sync::mpsc::Receiver<Control>,
    store: Database,
    cancel: Arc<AtomicBool>,
    tx: async_channel::Sender<Update>,
) {
    if summary.cwd.starts_with("remote://") {
        remote::run(summary, prompt, settings, controls, store, cancel, tx);
        return;
    }
    run_turn_using(
        TurnInput {
            summary,
            prompt,
            settings,
        },
        controls,
        store,
        cancel,
        tx,
        provider::resolve,
    );
}

fn run_turn_using(
    input: TurnInput,
    controls: std::sync::mpsc::Receiver<Control>,
    store: Database,
    cancel: Arc<AtomicBool>,
    tx: async_channel::Sender<Update>,
    resolve_binary: impl FnOnce(Kind) -> Option<std::path::PathBuf>,
) {
    let TurnInput {
        summary,
        prompt,
        mut settings,
    } = input;
    if settings.account.root.is_none() && settings.account.id != monocode_core::accounts::DEFAULT {
        settings.account.root = locked(&store)
            .ok()
            .and_then(|s| s.data_dir().map(std::path::Path::to_path_buf));
    }
    let source_account = settings.account.id.clone();
    let user_request = prompt.clone();
    let mut transcript = Transcript::default();
    let mut history_loaded = false;
    let shared = locked(&store)
        .map(|s| s.uses_monocode_schema())
        .unwrap_or(false);
    let mut pending = PendingSave::new(shared, std::time::Instant::now());
    pending.own_context(&summary.provider, &source_account);
    let mut initial_delivery = None;
    let mut accepted_messages = Vec::new();
    let mut provider_id = summary.provider_id.clone();
    let mut turn_start = 0;
    let turn_started_ms = monocode_core::time::now_ms();
    let mut planning = false;
    let mut images_wanted = false;
    let mut turn_receipt: Option<(usize, monocode_core::agent::TurnInfo)> = None;
    let mut mono_fingerprint = None;
    // Context sent once per provider conversation (`introduction`), with
    // whether this turn sent it.
    let mut introductions: Vec<(monocode_core::introduction::Introduction, bool)> = Vec::new();
    let mut compacted = false;
    let mut prepared_handoff = None;
    let mut handoff_accepted = false;
    // SessionReview's recorded turn: its project and id, once the baseline is taken.
    let mut checkpoint: Option<(String, String)> = None;
    let mut created_worktree = None;
    let mut context = locked(&store)
        .ok()
        .and_then(|s| s.context_usage(summary.id).ok().flatten())
        .unwrap_or_default();
    let mut usage_limit = None;
    let result = (|| {
        if let Some(reason) = locked(&store)?.edit_recovery_reason(summary.id)? {
            return Err(reason);
        }
        if settings.worktree_removed {
            return Err("Select a branch or worktree to continue this conversation.".into());
        }
        if settings.account.id != monocode_core::accounts::DEFAULT
            && !locked(&store)?
                .provider_accounts()?
                .exists(Kind::from_id(&summary.provider), &settings.account.id)
        {
            return Err(
                "This provider account was removed. Choose another account to continue.".into(),
            );
        }
        transcript = locked(&store)?.load(summary.id)?;
        history_loaded = true;
        turn_start = transcript.rows.len();
        settings.operator_enabled |= settings.operator;
        let mut persisted = settings.clone();
        persisted.handoff = None;
        // These modes belong to this turn, not the next composer draft.
        persisted.plan = false;
        persisted.operator = false;
        persisted.orchestrator = false;
        let start = transcript.begin(if settings.internal_message {
            "mono-system"
        } else {
            "user"
        });
        if let Some(at) = settings
            .turn_model
            .as_ref()
            .and_then(|turn| turn.started_at)
        {
            transcript.rows[start].sent_at = Some(at);
        }
        if settings.mono.is_some()
            && let Some(at) = settings.mono_sent_at
        {
            transcript.rows[start].sent_at = Some(at);
        }
        let started_at = transcript.rows[start].sent_at;
        transcript.append(&prompt);
        transcript.attach_files(&settings.attachments);
        // Show the working receipt while connecting, before the first token.
        transcript.begin("assistant");
        // Keep the already painted prompt in the failure replay even if saving
        // the turn's configuration fails before its first transcript write.
        locked(&store)?.configure_agent(summary.id, &summary.provider, &persisted)?;
        locked(&store)?.write_rows_with_message(
            summary.id,
            start,
            &transcript.rows[start..],
            settings.mono_message_id.as_deref(),
        )?;
        let mut info =
            settings
                .turn_model
                .clone()
                .unwrap_or_else(|| monocode_core::agent::TurnInfo {
                    provider: summary.provider.clone(),
                    model_id: settings.model.clone(),
                    ..Default::default()
                });
        info.started_at = info.started_at.or(started_at);
        // The receipt's key follows all continuation rows of the prompt.
        let turn = transcript
            .rows
            .iter()
            .rposition(Row::is_prompt)
            .map_or(0, |at| at + 1);
        persistence::save_turn_receipt(&store, summary.id, turn, info.clone())?;
        turn_receipt = Some((turn, info));
        initial_delivery = settings.mono_message_id.clone();
        let creating_worktree = settings.new_worktree && settings.worktree_cwd.is_none();
        publish(
            &tx,
            &cancel,
            Update {
                worktree_cwd: None,
                commands: None,
                provider_turn_id: None,
                turn_ready: false,
                follow_up_delivered: Vec::new(),
                follow_up_errors: Vec::new(),
                delivered: settings.mono_message_id.clone(),
                start,
                rows: transcript.rows[start..].to_vec(),
                provider_id: None,
                model: None,
                config_model: None,
                config_settings: None,
                title: None,
                context: None,
                metrics: None,
                usage_limit: None,
                background: None,
                approvals: Vec::new(),
                approvals_resolved: Vec::new(),
                questions: Vec::new(),
                questions_resolved: Vec::new(),
                handoff: None,
                status: if creating_worktree {
                    "Creating worktree…"
                } else if settings.pending_provider_switch.is_some() {
                    "Preparing handoff…"
                } else {
                    "Connecting…"
                }
                .into(),
                finished: false,
            },
        )?;
        if creating_worktree {
            let path = monocode_core::git::create_worktree(
                std::path::Path::new(&summary.cwd),
                settings.worktree_base.as_deref().unwrap_or("HEAD"),
                summary.id,
            )?;
            let cwd = path.to_string_lossy().into_owned();
            created_worktree = Some(cwd.clone());
            settings.worktree_cwd = Some(cwd.clone());
            settings.new_worktree = false;
            {
                let store = locked(&store)?;
                let mut persisted = store.agent_settings(summary.id)?.unwrap_or_default();
                persisted.worktree_cwd = Some(cwd.clone());
                persisted.new_worktree = false;
                store.save_agent_settings(summary.id, &persisted)?;
            }
            publish(
                &tx,
                &cancel,
                Update {
                    worktree_cwd: Some(cwd),
                    commands: None,
                    provider_turn_id: None,
                    turn_ready: false,
                    follow_up_delivered: Vec::new(),
                    follow_up_errors: Vec::new(),
                    delivered: None,
                    start: transcript.rows.len(),
                    rows: Vec::new(),
                    provider_id: None,
                    model: None,
                    config_model: None,
                    config_settings: None,
                    title: None,
                    context: None,
                    metrics: None,
                    usage_limit: None,
                    background: None,
                    approvals: Vec::new(),
                    approvals_resolved: Vec::new(),
                    questions: Vec::new(),
                    questions_resolved: Vec::new(),
                    handoff: None,
                    status: "Connecting…".into(),
                    finished: false,
                },
            )?;
        }
        if let Some(source) = settings.pending_provider_switch.take() {
            // The turn's first step: the outgoing model's recap can take a while.
            let mut transfer = monocode_core::tool::ToolCall::new(
                "provider-handoff",
                monocode_core::tool::HANDOFF,
            );
            let from = if source.model_label.is_empty() {
                Kind::from_id(&source.from).label()
            } else {
                source.model_label.as_str()
            };
            transfer.input = serde_json::json!({ "from": from }).to_string();
            let transfer_row = transcript.begin("tool");
            transcript.rows[transfer_row].text =
                serde_json::to_string(&transfer).map_err(|e| e.to_string())?;
            pending.changed(transfer_row);
            publish(
                &tx,
                &cancel,
                Update {
                    worktree_cwd: None,
                    commands: None,
                    provider_turn_id: None,
                    turn_ready: false,
                    follow_up_delivered: Vec::new(),
                    follow_up_errors: Vec::new(),
                    delivered: None,
                    start: transfer_row,
                    rows: transcript.rows[transfer_row..].to_vec(),
                    provider_id: None,
                    model: None,
                    config_model: None,
                    config_settings: None,
                    title: None,
                    context: None,
                    metrics: None,
                    usage_limit: None,
                    background: None,
                    approvals: Vec::new(),
                    approvals_resolved: Vec::new(),
                    questions: Vec::new(),
                    questions_resolved: Vec::new(),
                    handoff: None,
                    status: "Preparing handoff…".into(),
                    finished: false,
                },
            )?;
            let root = locked(&store)?.data_dir().map(std::path::Path::to_path_buf);
            let prepared = handoff::prepare(
                summary.id,
                &source,
                settings.worktree_cwd.as_deref().unwrap_or(&summary.cwd),
                &user_request,
                &transcript.rows[..turn_start],
                root,
                cancel.clone(),
            );
            transfer.state = if cancel.load(Ordering::Relaxed) {
                monocode_core::tool::ToolState::Cancelled
            } else {
                monocode_core::tool::ToolState::Completed
            };
            transcript.rows[transfer_row].text =
                serde_json::to_string(&transfer).map_err(|e| e.to_string())?;
            pending.changed(transfer_row);
            settings.handoff = Some(prepared.clone());
            prepared_handoff = Some(prepared.clone());
            provider_id = None;
            {
                let store = locked(&store)?;
                store.reset_provider_id(summary.id)?;
                let mut persisted = store.agent_settings(summary.id)?.unwrap_or_default();
                persisted.pending_provider_switch = None;
                persisted.handoff = Some(prepared.clone());
                store.save_agent_settings(summary.id, &persisted)?;
            }
            publish(
                &tx,
                &cancel,
                Update {
                    worktree_cwd: None,
                    commands: None,
                    provider_turn_id: None,
                    turn_ready: false,
                    follow_up_delivered: Vec::new(),
                    follow_up_errors: Vec::new(),
                    delivered: None,
                    start: transfer_row,
                    rows: transcript.rows[transfer_row..].to_vec(),
                    provider_id: None,
                    model: None,
                    config_model: None,
                    config_settings: None,
                    title: None,
                    context: None,
                    metrics: None,
                    usage_limit: None,
                    background: None,
                    approvals: Vec::new(),
                    approvals_resolved: Vec::new(),
                    questions: Vec::new(),
                    questions_resolved: Vec::new(),
                    handoff: Some(Some(prepared)),
                    status: "Connecting…".into(),
                    finished: false,
                },
            )?;
        }
        let kind = Kind::parse(&summary.provider)
            .ok_or_else(|| format!("Unknown provider: {}", summary.provider))?;
        let binary = resolve_binary(kind).ok_or_else(|| {
            format!(
                "{} Set MONOCODE_{}_BINARY to choose a different path.",
                kind.unavailable_hint(),
                kind.id().to_uppercase()
            )
        })?;
        // These providers parse slash arguments themselves. App context before
        // the command would turn it into an ordinary message instead.
        let prompt = settings.prepared_prompt.take().unwrap_or(prompt);
        let raw_native_command = matches!(kind, Kind::Omp | Kind::Devin)
            && provider::native_prompt(kind, &prompt)
                .trim_start()
                .starts_with('/');
        let prompt = if raw_native_command {
            prompt
        } else if let Some(mono) = &settings.mono {
            let files = monocode_core::mono::load_files(&mono.root, &mono.mono)?;
            let rotated = provider_id.is_some() && context.ratio().is_some_and(|r| r >= 0.8);
            if rotated {
                provider_id = None;
                locked(&store)?.reset_provider_id(summary.id)?;
            }
            let receipt = locked(&store)?.setting(&format!("mono-context:{}", summary.id))?;
            let rules = monocode_core::mono::files::content_hash(
                &monocode_core::mono::instructions(&mono.mono, &mono.executable),
            );
            let held_rules = receipt
                .as_deref()
                .and_then(|r| serde_json::from_str::<serde_json::Value>(r).ok())
                .and_then(|r| r["rules"].as_str().map(str::to_owned));
            let mut added = monocode_core::mono::context(
                &mono.mono,
                &files,
                &mono.executable,
                monocode_core::mono::rules_in_system(
                    kind,
                    provider_id.is_some(),
                    held_rules.as_deref(),
                    &rules,
                ),
            );
            // A workstream's orchestrator reads how to plan, split the work
            // across agents in worktrees and ship it as pull requests.
            if let Some(workstream) = &mono.mono.workstream {
                let brief = monocode_core::mono::workstreams::brief(&mono.mono.label(), workstream);
                added = format!("{added}\n\n{brief}");
            }
            // A group's lead reads how to run its room, a member in its
            // seat where its words go; handed again when either changes.
            if let Some(group) = mono.groups.brief(&mono.mono) {
                added = format!("{added}\n\n{group}");
            }
            let fingerprint = monocode_core::mono::files::content_hash(&added);
            mono_fingerprint = Some((fingerprint.clone(), rules.clone()));
            let expected =
                serde_json::json!({"provider":provider_id,"hash":fingerprint,"rules":rules})
                    .to_string();
            let intro = if provider_id.is_none() || receipt.as_deref() != Some(&expected) {
                added
            } else {
                String::new()
            };
            let brief = if provider_id.is_none() && turn_start > 0 {
                monocode_core::mono::rotation_brief(&transcript.rows[..turn_start])
            } else {
                String::new()
            };
            let prompt = if let Some(handoff) = &settings.handoff {
                monocode_core::response::wrap_handoff_prompt(&handoff.from, &handoff.brief, &prompt)
            } else {
                prompt
            };
            // Members' answers to an @Name or an errand went around the
            // lead; it reads them once, with the next message.
            let posts = if mono.groups.group.is_some() {
                let context = monocode_core::mono::groups::native::posts_context(
                    &transcript.rows[..turn_start],
                );
                if context.is_empty() {
                    context
                } else {
                    format!("{context}\n\n")
                }
            } else {
                String::new()
            };
            // Outside the hashed context, so the clock never resends it.
            let now = chrono::Local::now().format("%A %-d %B %Y, %H:%M");
            format!("{intro}\n\n{brief}\n\n{posts}<local_time>{now}</local_time>\n{prompt}")
        } else if let Some(handoff) = &settings.handoff {
            monocode_core::response::wrap_handoff_prompt(&handoff.from, &handoff.brief, &prompt)
        } else if provider_id.is_none() && turn_start > 0 {
            let mut history = String::new();
            for row in transcript.rows[..turn_start]
                .iter()
                .rev()
                .filter(|r| matches!(r.role.as_str(), "user" | "assistant"))
            {
                if history.len() + row.text.len() > 32_000 {
                    break;
                }
                history = format!("{}: {}\n\n{history}", row.role, row.text);
            }
            format!(
                "Continue this conversation, using the prior messages as context.\n\n{history}\nCurrent user message:\n{prompt}"
            )
        } else {
            prompt
        };
        let cwd = settings.worktree_cwd.as_deref().unwrap_or(&summary.cwd);
        if settings.mono.is_none()
            && cwd != "~"
            && let Some(store) = monocode_core::checkpoint::store()
        {
            let id = monocode_core::checkpoint::new_turn_id();
            let session = summary.id.to_string();
            // Capture errors are persisted and shown in the review.
            let _ = store.exclusive(|store| {
                // An interrupted turn continues with this session's edit evidence.
                store.capture(&session, cwd, &edited_paths(&transcript.rows[..turn_start]))?;
                store.begin_turn(&session, cwd, &id)
            });
            checkpoint = Some((cwd.to_owned(), id));
        }
        let mut prompt = provider::prepare_prompt(kind, &prompt, cwd, &settings.attachments)?;
        if !raw_native_command && prompt.contains("@note:") {
            let notes = locked(&store)?.notes()?;
            for note in notes {
                let token = format!("@note:{}", note.id);
                if prompt
                    .lines()
                    .filter(|line| !line.trim_start().starts_with('>'))
                    .flat_map(str::split_whitespace)
                    .any(|word| word.trim_end_matches([',', '.', ';']) == token)
                {
                    prompt = format!(
                        "Referenced note: {}\n<note>\n{}\n</note>\n\n{prompt}",
                        note.title,
                        monocode_core::response::limit(&note.body, 64_000)
                    );
                }
            }
        }
        if !raw_native_command && settings.orchestrator {
            prompt = format!(
                "{}\n\nUser request:\n{prompt}",
                monocode_core::orchestration::INSTRUCTIONS
            );
        }
        // The Operator rules go once per provider conversation, not with
        // every message; Claude reads them in its system prompt instead.
        if !raw_native_command
            && settings.operator_enabled
            && !monocode_core::app_access::operator_in_system(kind)
        {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let rules = monocode_core::app_access::operator_instructions(&executable);
            let introduction = monocode_core::introduction::Introduction::new(
                format!(
                    "operator-rules:{}:{}:{}",
                    summary.id,
                    kind.id(),
                    settings.account.id
                ),
                serde_json::json!({"rules": monocode_core::mono::files::content_hash(&rules)}),
            );
            let due = introduction.due(&*locked(&store)?, provider_id.as_deref())?;
            if due {
                prompt = format!("{rules}\n\n{prompt}");
            }
            introductions.push((introduction, due));
        }
        // A coding chat the user lets start swarms learns how, once per
        // provider conversation.
        if !raw_native_command && settings.swarm_offer && !settings.orchestrator {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            let rules =
                monocode_core::orchestration::control::swarm_offer(&executable.to_string_lossy());
            let introduction = monocode_core::introduction::Introduction::new(
                format!(
                    "swarm-offer:{}:{}:{}",
                    summary.id,
                    kind.id(),
                    settings.account.id
                ),
                serde_json::json!({"rules": monocode_core::mono::files::content_hash(&rules)}),
            );
            let due = introduction.due(&*locked(&store)?, provider_id.as_deref())?;
            if due {
                prompt = format!("{rules}\n\n{prompt}");
            }
            introductions.push((introduction, due));
        }
        // Every foreground session receives an independent visual capability.
        // This happens after configuration was persisted, and Settings' env is
        // transport-only, so credentials never enter saved conversation settings.
        let visual_bridge = if !settings.text_only && !raw_native_command {
            Some(monocode_core::visual_bridge::Server::start(summary.id)?)
        } else {
            None
        };
        if let Some(visual_bridge) = &visual_bridge {
            let executable = std::env::current_exe().map_err(|e| e.to_string())?;
            settings
                .environment
                .extend(visual_bridge.environment(&executable));
            if let Some(discovery) = visual_discovery::Discovery::for_context(
                summary.id,
                kind,
                &settings.account.id,
                provider_id.as_deref(),
                &executable,
            ) {
                let due = discovery.due(&*locked(&store)?, provider_id.as_deref())?;
                if due {
                    prompt = discovery.prepend(prompt);
                }
                introductions.push((discovery.into_introduction(), due));
            }
        }
        let request = Request {
            kind,
            binary,
            cwd: settings
                .worktree_cwd
                .as_deref()
                .unwrap_or(&summary.cwd)
                .into(),
            prompt,
            provider_id: provider_id.clone(),
            model: settings.model.clone(),
            settings,
        };
        let mut tool_rows = std::collections::HashMap::new();
        // A plan turn's answer can become its plan once the turn has ended.
        planning = request.settings.plan;
        let antigravity = request.kind == Kind::Antigravity;
        let mut plan_tools = std::collections::HashSet::new();
        // A turn going round in circles is stopped (`loop_guard`): each call
        // once it has finished, and the answer as it grows. A provider that takes a
        // steer is reminded first, on the same channel as the user's controls.
        let (mut guard, reminders, controls) = if request.kind.can_steer() {
            let (reminders, controls) = Reminders::join(controls);
            (
                monocode_core::loop_guard::Guard::reminding(),
                Some(reminders),
                controls,
            )
        } else {
            (monocode_core::loop_guard::Guard::default(), None, controls)
        };
        let mut reminded = 0;
        let mut turn_text = String::new();
        // The running-work list attributes the provider and its tools to this session.
        let result = monocode_core::process::owned_by(summary.id, || {
            provider::run_with_controls(request, cancel.clone(), Some(controls), |events| {
                // Cancellation freezes the transcript, even while the native
                // provider drains its interrupt acknowledgement.
                if cancel.load(Ordering::Relaxed) {
                    return Err("Stopped".into());
                }
                let publications = visual_bridge
                    .as_ref()
                    .map(|bridge| bridge.drain())
                    .unwrap_or_default();
                if events.is_empty() && publications.is_empty() {
                    return Ok(());
                }
                let mut start = transcript.rows.len();
                let mut status = "Streaming…".to_string();
                let mut changed_id = None;
                let mut model = None;
                let mut config_model = None;
                let mut config_settings = None;
                let mut context_changed = false;
                let mut metrics = None;
                let mut background = None;
                let mut approvals = Vec::new();
                let mut approvals_resolved = Vec::new();
                let mut questions = Vec::new();
                let mut questions_resolved = Vec::new();
                let mut handoff = None;
                let mut turn_ready = false;
                let mut follow_up_delivered = Vec::new();
                let mut follow_up_errors = Vec::new();
                let mut boundary = false;
                let mut commands = None;
                let mut provider_turn_id = None;
                for event in events {
                    match event {
                        Event::Commands(discovered) => commands = Some(discovered),
                        Event::TurnId(id) => {
                            if let Some((_, info)) = &mut turn_receipt {
                                info.provider_turn_id = Some(id.clone());
                            }
                            provider_turn_id = Some(id);
                        }
                        Event::Ready => {
                            turn_ready = true;
                            boundary = true;
                            if prepared_handoff.is_some() && !handoff_accepted {
                                handoff_accepted = true;
                                handoff = Some(None);
                                let store = locked(&store)?;
                                let mut persisted =
                                    store.agent_settings(summary.id)?.unwrap_or_default();
                                persisted.handoff = None;
                                store.save_agent_settings(summary.id, &persisted)?;
                            }
                        }
                        // The loop guard's reminders are not the user's words.
                        Event::FollowUp(message)
                            if monocode_core::loop_guard::is_reminder(&message.id) => {}
                        Event::FollowUpFailed { id, .. }
                            if monocode_core::loop_guard::is_reminder(&id) => {}
                        Event::FollowUp(message) => {
                            let at = transcript.begin("user");
                            transcript.rows[at].sent_at =
                                message.sent_at.or(transcript.rows[at].sent_at);
                            start = start.min(at);
                            transcript.append(&message.text);
                            transcript.attach_files(&message.attachments);
                            // Paint the working receipt with the accepted steer,
                            // even when the provider has no new output yet.
                            transcript.begin("assistant");
                            let turn = transcript
                                .rows
                                .iter()
                                .rposition(Row::is_prompt)
                                .map_or(0, |at| at + 1);
                            pending.accept(
                                &store,
                                summary.id,
                                at,
                                &transcript,
                                &message.id,
                                std::time::Instant::now(),
                            )?;
                            if let Some((root_turn, root)) = &turn_receipt {
                                // A steer keeps its send time and model identity,
                                // while timing/metrics belong to the original run.
                                let steer = monocode_core::agent::TurnInfo {
                                    joined_turn: Some(*root_turn),
                                    provider: root.provider.clone(),
                                    model: root.model.clone(),
                                    model_id: root.model_id.clone(),
                                    provider_turn_id: root.provider_turn_id.clone(),
                                    ..Default::default()
                                };
                                persistence::save_turn_receipt(&store, summary.id, turn, steer)?;
                            }
                            accepted_messages.push(message.id.clone());
                            follow_up_delivered.push(message.id);
                        }
                        Event::FollowUpFailed { id, error } => follow_up_errors.push((id, error)),
                        Event::BeginAssistant => {
                            if transcript
                                .rows
                                .last()
                                .is_none_or(|r| r.role != "assistant" || !r.text.is_empty())
                            {
                                let at = transcript.begin("assistant");
                                start = start.min(at);
                                pending.changed(at);
                            }
                        }
                        Event::Text(text) => {
                            if transcript.rows.last().is_none_or(|r| r.role != "assistant") {
                                start = start.min(transcript.begin("assistant"));
                            }
                            turn_text.push_str(&text);
                            let at = transcript.append(&text);
                            start = start.min(at);
                            pending.changed(at);
                            if let Some(reason) = guard.text(&turn_text) {
                                return Err(reason.into());
                            }
                        }
                        Event::Reasoning(text) => {
                            if text.is_empty() {
                                continue;
                            }
                            if transcript.rows.last().is_none_or(|r| r.role != "reasoning") {
                                start = start.min(transcript.begin("reasoning"));
                            }
                            let at = transcript.append(&text);
                            start = start.min(at);
                            pending.changed(at);
                        }
                        Event::Plan(text) => {
                            let at = plan_row(&mut transcript, text)?;
                            start = start.min(at);
                            pending.changed(at);
                            boundary = true;
                        }
                        Event::Image(image) => {
                            let at = transcript.begin(monocode_core::images::REPLY_ROLE);
                            transcript
                                .append(&serde_json::to_string(&image).map_err(|e| e.to_string())?);
                            start = start.min(at);
                            pending.changed(at);
                            boundary = true;
                        }
                        Event::Tool(tool) => {
                            if tool.name == "contextCompaction"
                                && tool.state == monocode_core::tool::ToolState::Completed
                            {
                                compacted = true;
                            }
                            match guard.call(&tool) {
                                Some(monocode_core::loop_guard::Repeat::Stop(reason)) => {
                                    return Err(reason.into());
                                }
                                Some(monocode_core::loop_guard::Repeat::Remind(text)) => {
                                    if let Some(reminders) = &reminders {
                                        reminded += 1;
                                        reminders.send(reminded, text);
                                        status = "Reminded the agent that it keeps repeating the same call".into();
                                    }
                                }
                                None => {}
                            }
                            if antigravity
                                && !images_wanted
                                && monocode_core::images::update_makes_image(
                                    &serde_json::to_string(&tool).unwrap_or_default(),
                                )
                            {
                                images_wanted = true;
                            }
                            // Claude hands its plan back through ExitPlanMode.
                            let plan = monocode_core::plan::is_exit_plan_mode(&tool.name)
                                .then(|| monocode_core::plan::exit_plan_mode_plan(&tool.input))
                                .flatten()
                                .filter(|_| plan_tools.insert(tool.id.clone()));
                            if tool.is_compaction() {
                                status = if tool.state == monocode_core::tool::ToolState::Running {
                                    "Compacting context…"
                                } else {
                                    "Working…"
                                }
                                .into();
                            }
                            let ix = *tool_rows
                                .entry(tool.id.clone())
                                .or_insert_with(|| transcript.begin("tool"));
                            transcript.rows[ix].text =
                                serde_json::to_string(&tool).map_err(|e| e.to_string())?;
                            start = start.min(ix);
                            pending.changed(ix);
                            contributions::record(&store, summary.id, &summary.cwd, &tool);
                            if let Some(text) = plan {
                                let at = plan_row(&mut transcript, text)?;
                                start = start.min(at);
                                pending.changed(at);
                                boundary = true;
                            }
                        }
                        Event::SessionId(id) => {
                            if provider_id.as_ref() != Some(&id) {
                                let store = locked(&store)?;
                                let mut settings =
                                    store.agent_settings(summary.id)?.unwrap_or_default();
                                if let Some(pending) = settings.pending_provider_switch.as_mut()
                                    && pending.from == summary.provider
                                    && pending.account.id == source_account
                                {
                                    pending.provider_id = Some(id.clone());
                                    store.save_agent_settings(summary.id, &settings)?;
                                } else if settings.account.id == source_account
                                    && store.sessions()?.iter().any(|session| {
                                        session.id == summary.id
                                            && session.provider == summary.provider
                                    })
                                {
                                    store.provider_id(summary.id, &id)?;
                                }
                                provider_id = Some(id.clone());
                                changed_id = Some(id);
                            }
                        }
                        Event::Model(id) => model = Some(id),
                        Event::ConfigChanged {
                            model: changed_model,
                            model_settings,
                        } => {
                            let store = locked(&store)?;
                            let mut settings =
                                store.agent_settings(summary.id)?.unwrap_or_default();
                            if let Some(pending) = settings.pending_provider_switch.as_mut()
                                && pending.from == summary.provider
                                && pending.account.id == source_account
                            {
                                if let Some(id) = &changed_model {
                                    pending.model = Some(id.clone());
                                }
                                pending.model_settings.extend(model_settings.clone());
                            } else if settings.account.id == source_account
                                && store.sessions()?.iter().any(|session| {
                                    session.id == summary.id && session.provider == summary.provider
                                })
                            {
                                if let Some(id) = &changed_model {
                                    settings.model = Some(id.clone());
                                }
                                settings.model_settings.extend(model_settings.clone());
                            }
                            store.save_agent_settings(summary.id, &settings)?;
                            if changed_model.is_some() {
                                model = changed_model.clone();
                                config_model = changed_model;
                            }
                            config_settings
                                .get_or_insert_with(std::collections::HashMap::new)
                                .extend(model_settings);
                        }
                        Event::Interjection {
                            text,
                            custom_type,
                            severity,
                        } => {
                            let note = monocode_core::transcript::Interjection {
                                text,
                                custom_type,
                                severity,
                            };
                            let at = transcript.begin("interjection");
                            transcript
                                .append(&serde_json::to_string(&note).map_err(|e| e.to_string())?);
                            start = start.min(at);
                            pending.changed(at);
                            boundary = true;
                        }
                        Event::Metrics(reading) => {
                            if let Some((_, info)) = &mut turn_receipt {
                                info.metrics = Some(reading);
                            }
                            metrics = Some(reading);
                        }
                        Event::Context { used, window } => {
                            context.merge(used, window);
                            context_changed = true;
                            pending.context_changed();
                        }
                        Event::Approval(approval) => {
                            status = "Waiting for approval".into();
                            approvals.push(approval);
                            boundary = true;
                        }
                        Event::ApprovalResolved(id) => {
                            approvals_resolved.push(id);
                            boundary = true;
                        }
                        Event::Question(question) => {
                            status = "Waiting for your answer".into();
                            questions.push(question);
                            boundary = true;
                        }
                        Event::QuestionResolved(id) => {
                            questions_resolved.push(id);
                            boundary = true;
                        }
                        Event::Status(text) => status = text,
                        Event::Background(tasks) => background = Some(tasks),
                        Event::UsageLimited(limit) => usage_limit = Some(limit),
                        Event::Done => {
                            status = "Ready".into();
                            boundary = true;
                        }
                    }
                }
                for publication in &publications {
                    let document =
                        serde_json::to_string(&publication.document).map_err(|e| e.to_string())?;
                    let at = transcript.begin("visual");
                    transcript.append(&document);
                    start = start.min(at);
                    pending.changed(at);
                    boundary = true;
                }
                let now = std::time::Instant::now();
                let publication_result = (|| {
                    if pending.due(boundary, now) {
                        pending.flush(&store, summary.id, &transcript, &context, now)?;
                    }
                    publish(
                        &tx,
                        &cancel,
                        Update {
                            worktree_cwd: None,
                            commands,
                            provider_turn_id,
                            turn_ready,
                            follow_up_delivered,
                            follow_up_errors,
                            delivered: None,
                            start,
                            rows: transcript.rows[start..].to_vec(),
                            provider_id: changed_id,
                            model,
                            config_model,
                            config_settings,
                            title: None,
                            context: context_changed.then_some(context),
                            metrics,
                            usage_limit: None,
                            background,
                            approvals,
                            approvals_resolved,
                            questions,
                            questions_resolved,
                            handoff,
                            status,
                            finished: false,
                        },
                    )
                })();
                // A tool succeeds only after both durable storage and the live
                // transcript update have accepted the visual.
                for publication in publications {
                    let result = publication_result.as_ref().map(|_| {
                        serde_json::json!({"id":publication.document.id,"title":publication.document.title,"saved":true})
                    }).map_err(Clone::clone);
                    let _ = publication.reply.send(result);
                }
                publication_result
            })
        });
        if result.is_ok()
            && let Some((fingerprint, rules)) = &mono_fingerprint
        {
            locked(&store)?.save_setting(
                &format!("mono-context:{}", summary.id),
                &serde_json::json!({"provider":provider_id,"hash":fingerprint,"rules":rules})
                    .to_string(),
            )?;
        }
        if compacted {
            // The provider summarized its conversation and may have dropped
            // the context it was given once: the next turn sends it again.
            let held = locked(&store)?;
            for (introduction, _) in &introductions {
                introduction.forget(&held)?;
            }
            if mono_fingerprint.is_some() {
                held.save_setting(&format!("mono-context:{}", summary.id), "")?;
            }
        } else if result.is_ok() && !cancel.load(Ordering::Relaxed) {
            let held = locked(&store)?;
            for (introduction, sent) in &introductions {
                if *sent {
                    introduction.remember(&held, provider_id.as_deref())?;
                }
            }
        }
        result
    })();
    let cancelled =
        cancel.load(Ordering::Relaxed) || result.as_ref().is_err_and(|error| error == "Stopped");
    let handoff = prepared_handoff.map(|mut ready| {
        if handoff_accepted {
            None
        } else {
            // This accepted user message never reached the incoming provider.
            // Carry it with the ready packet when the user continues later.
            if !user_request.trim().is_empty() {
                ready
                    .brief
                    .push_str("\n\nAfter the switch, before this message, the user also sent:\n\n");
                ready.brief.push_str(&user_request);
            }
            Some(ready)
        }
    });
    let handoff_save = if let Some(ready) = &handoff {
        locked(&store).and_then(|store| {
            let mut persisted = store.agent_settings(summary.id)?.unwrap_or_default();
            persisted.pending_provider_switch = None;
            persisted.handoff = ready.clone();
            store.save_agent_settings(summary.id, &persisted)
        })
    } else {
        Ok(())
    };
    // A storage error or cancelled publish can leave this turn's latest rows
    // unseen by the UI. Replay the turn on failure before sending an error or
    // settled tools; starting after those missing rows would abort in patch().
    let mut final_start = if result.is_err() {
        turn_start
    } else {
        transcript.rows.len()
    };
    for (ix, row) in transcript.rows.iter_mut().enumerate().skip(turn_start) {
        if row.role != "tool" {
            continue;
        }
        if let Ok(mut tool) = serde_json::from_str::<monocode_core::tool::ToolCall>(&row.text)
            && tool.state == monocode_core::tool::ToolState::Running
        {
            tool.state = if cancelled {
                monocode_core::tool::ToolState::Cancelled
            } else {
                monocode_core::tool::ToolState::Failed
            };
            if tool.output.is_empty() {
                tool.output = if cancelled {
                    "Cancelled".into()
                } else {
                    result.as_ref().err().cloned().unwrap_or_else(|| {
                        "No completion result was received before the turn ended.".into()
                    })
                };
            }
            row.text = serde_json::to_string(&tool).unwrap();
            final_start = final_start.min(ix);
        }
    }
    if history_loaded && result.is_ok() && !cancelled {
        if images_wanted {
            // Gemini's image subagent can still be writing when the turn ends.
            let _ = tx.send_blocking(Update::status(
                transcript.rows.len(),
                "Waiting for Gemini's pictures…",
            ));
            let found = match monocode_core::skills::bundled::data_dir()
                .zip(monocode_core::accounts::home())
            {
                Some((data, home)) => monocode_core::images::await_turn_images(
                    std::time::Duration::from_secs(120),
                    std::time::Duration::from_secs(2),
                    || cancel.load(Ordering::Relaxed),
                    || {
                        monocode_core::images::collect_images_since(
                            &home.join(monocode_core::images::BRAIN_DIR),
                            &data.join(monocode_core::images::GENERATED_IMAGE_DIR),
                            turn_started_ms,
                        )
                    },
                    std::thread::sleep,
                ),
                None => Vec::new(),
            };
            for image in found {
                let at = transcript.begin(monocode_core::images::REPLY_ROLE);
                transcript.append(&serde_json::to_string(&image).unwrap_or_default());
                final_start = final_start.min(at);
            }
        }
        if planning && let Some(plan) = monocode_core::plan::promote(&transcript.rows[turn_start..])
        {
            let at = transcript.begin(monocode_core::plan::ROLE);
            transcript.append(&serde_json::to_string(&plan).unwrap_or_default());
            final_start = final_start.min(at);
        }
    }
    if let Some((cwd, id)) = &checkpoint
        && let Some(store) = monocode_core::checkpoint::store()
    {
        // The provider has exited, so this snapshot is the turn's settled result.
        let session = summary.id.to_string();
        let _ = store.exclusive(|store| {
            store.capture(&session, cwd, &edited_paths(&transcript.rows[turn_start..]))?;
            store.finish_turn(&session, cwd, id)
        });
    }
    let (start, rows, mut status) = if !history_loaded {
        // A rejected preflight has no row baseline. Writing an error at row 0
        // would replace a real conversation's first saved message.
        (
            0,
            Vec::new(),
            if cancelled {
                "Ready".into()
            } else {
                result
                    .err()
                    .unwrap_or_else(|| "Conversation was not loaded".into())
            },
        )
    } else if let Err(error) = result
        && !cancelled
    {
        let start = final_start.min(transcript.begin("error"));
        transcript.append(&error);
        pending.changed(start);
        let error = match pending.flush(
            &store,
            summary.id,
            &transcript,
            &context,
            std::time::Instant::now(),
        ) {
            Ok(()) => error,
            Err(storage) => format!("{error} · Saving failed: {storage}"),
        };
        (start, transcript.rows[start..].to_vec(), error)
    } else {
        if final_start < transcript.rows.len() {
            pending.changed(final_start);
        }
        let status = pending
            .flush(
                &store,
                summary.id,
                &transcript,
                &context,
                std::time::Instant::now(),
            )
            .map(|_| "Ready".into())
            .unwrap_or_else(|error| format!("Saving failed: {error}"));
        (final_start, transcript.rows[final_start..].to_vec(), status)
    };
    // Settle provenance in storage before notifying the view, including when
    // its window has closed or the provider was cancelled.
    if let Some((turn, mut info)) = turn_receipt {
        let completed = monocode_core::time::now_ms();
        info.duration_ms = info.started_at.map_or(0, |at| completed.saturating_sub(at));
        info.duration_known = true;
        info.completed_at = info.started_at.map(|_| completed);
        if let Err(error) = persistence::save_turn_receipt(&store, summary.id, turn, info) {
            status = format!("{status} · Saving turn metadata failed: {error}");
        }
    }
    if let Err(error) = handoff_save {
        status = format!("{status} · Saving handoff failed: {error}");
    }
    // Final status must still arrive after cancellation. Dropping a window closes
    // the receiver, so the worker never keeps a closed viewport alive.
    let _ = tx.send_blocking(Update {
        // A stop can make the UI skip intermediate updates. Keep the created
        // checkout in the final receipt so the next Send cannot create it twice.
        worktree_cwd: created_worktree,
        commands: None,
        provider_turn_id: None,
        turn_ready: false,
        follow_up_delivered: accepted_messages,
        follow_up_errors: Vec::new(),
        delivered: initial_delivery,
        start,
        rows,
        provider_id,
        model: None,
        config_model: None,
        config_settings: None,
        title: None,
        context: None,
        metrics: None,
        usage_limit: if cancelled { None } else { usage_limit },
        background: None,
        approvals: Vec::new(),
        approvals_resolved: Vec::new(),
        questions: Vec::new(),
        questions_resolved: Vec::new(),
        handoff,
        status,
        finished: true,
    });
}

/// checkpointEdits.ts's sessionEditPaths: files named by successful edit tools.
pub(crate) fn edited_paths(rows: &[Row]) -> Vec<String> {
    let mut paths = Vec::new();
    for row in rows.iter().filter(|r| r.role == "tool") {
        let Ok(tool) = serde_json::from_str::<monocode_core::tool::ToolCall>(&row.text) else {
            continue;
        };
        if tool.state != monocode_core::tool::ToolState::Completed {
            continue;
        }
        let shown = tool.presentation();
        if matches!(
            shown.action.as_str(),
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
        ) {
            let input = serde_json::from_str::<serde_json::Value>(&tool.input).unwrap_or_default();
            let many = input["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|path| path.as_str());
            let desktop = tool.display.as_ref().unwrap_or(&input["__desktop"]);
            let desktop_paths = desktop["paths"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|path| path.as_str());
            for path in shown
                .file_path
                .as_deref()
                .into_iter()
                .chain(many)
                .chain(desktop_paths)
            {
                if !path.is_empty() && !paths.iter().any(|p| p == path) {
                    paths.push(path.to_owned());
                }
            }
        }
    }
    paths
}

fn shlex_quote(text: &str) -> String {
    monocode_core::app_access::shell_word(text)
}

#[cfg(test)]
mod edited_paths_tests {
    use super::*;
    use monocode_core::tool::{ToolCall, ToolState};
    use serde_json::json;

    #[test]
    fn successful_edits_keep_real_paths_across_the_whole_session() {
        let row = |name: &str, state, input: serde_json::Value| {
            let mut tool = ToolCall::new("tool", name);
            tool.state = state;
            tool.input = input.to_string();
            Row {
                role: "tool".into(),
                text: serde_json::to_string(&tool).unwrap(),
                first: true,
                sent_at: None,
            }
        };
        let rows = vec![
            row(
                "Edit",
                ToolState::Completed,
                json!({"file_path":"/repo/a.rs", "cwd":"/repo"}),
            ),
            row(
                "Read",
                ToolState::Completed,
                json!({"file_path":"/repo/read.rs"}),
            ),
            row(
                "Write",
                ToolState::Failed,
                json!({"file_path":"/repo/failed.rs"}),
            ),
            row(
                "Edit",
                ToolState::Running,
                json!({"file_path":"/repo/running.rs"}),
            ),
            Row {
                role: "user".into(),
                text: "A plain question".into(),
                first: true,
                sent_at: None,
            },
            row(
                "fileChange",
                ToolState::Completed,
                json!({"path":"/repo/a.rs", "paths":["/repo/a.rs", "relative.rs"]}),
            ),
        ];
        assert_eq!(edited_paths(&rows), ["/repo/a.rs", "relative.rs"]);
    }
}
