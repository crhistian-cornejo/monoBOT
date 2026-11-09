//! Settings › Voice: the local models (dictation's Parakeet and the calls'
//! Supertonic) with their size, download and delete, how a call talks (on
//! this machine, Gemini Live or Phonic), and the live providers' API keys,
//! which go to the system keychain and never come back to this page. All the
//! disk and keychain work runs on the background executor.
use crate::{
    input::{self, Input},
    settings::{
        SettingsView,
        widgets::{note, primary_button, secondary_button},
    },
    theme::*,
};
use gpui::{AnyElement, Context, Entity, SharedString, Task, div, prelude::*, px};
use monocode_core::dictation::{format_model_size, percent, progress};
use monocode_core::voice::{Provider, keys};
use std::collections::HashMap;

/// What the page shows about the disk and the keychain, read off-thread.
#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::settings) struct Snapshot {
    pub dictation: crate::dictation::Status,
    pub voice_installed: bool,
    pub voice_installing: bool,
    pub voice_on_disk: u64,
    pub keys: HashMap<&'static str, bool>,
}

/// A model the page can download or delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(in crate::settings) enum Model {
    Dictation,
    Voice,
}

impl Model {
    fn id(self) -> &'static str {
        match self {
            Self::Dictation => "dictation",
            Self::Voice => "voice",
        }
    }
}

#[derive(Default)]
pub(in crate::settings) struct VoiceState {
    pub snapshot: Option<Snapshot>,
    checked: bool,
    inputs: HashMap<&'static str, Entity<Input>>,
    /// A model downloading, with bytes done and total.
    pub downloading: Option<(Model, u64, u64)>,
    /// The delete waiting for its second click.
    pub confirming: Option<Model>,
    /// What the last action said, per row: an error or a confirmation.
    pub messages: HashMap<&'static str, String>,
    check: Option<Task<()>>,
    jobs: Vec<Task<()>>,
}

const PACES: [(&str, &str); 3] = [
    ("patient", "Patient"),
    ("normal", "Normal"),
    ("quick", "Quick"),
];
const LIVE: [(&str, &str); 3] = [
    ("local", "This Mac"),
    ("gemini", "Gemini Live"),
    ("phonic", "Phonic"),
];

fn read_snapshot(dictation: &dyn crate::dictation::Engine) -> Snapshot {
    use crate::monos::group_call::model as voice;
    Snapshot {
        dictation: dictation.status(),
        voice_installed: voice::installed(),
        voice_installing: voice::installing(),
        voice_on_disk: voice::disk_bytes(),
        keys: Provider::ALL
            .into_iter()
            .map(|provider| (provider.id(), keys::saved(provider)))
            .collect(),
    }
}

impl SettingsView {
    /// Reads the disk and the keychain again, off the interface thread.
    pub(in crate::settings) fn voice_refresh(&mut self, cx: &mut Context<Self>) {
        self.voice.checked = true;
        let engine = crate::dictation::engine(cx);
        let read = cx
            .background_executor()
            .spawn(async move { read_snapshot(engine.as_ref()) });
        self.voice.check = Some(cx.spawn(async move |this, cx| {
            let snapshot = read.await;
            let _ = this.update(cx, |this, cx| {
                this.voice.snapshot = Some(snapshot);
                cx.notify();
            });
        }));
    }

    fn voice_input(&mut self, provider: Provider, cx: &mut Context<Self>) -> Entity<Input> {
        if let Some(input) = self.voice.inputs.get(provider.id()) {
            return input.clone();
        }
        let key = provider.id();
        let input = cx.new(|cx| {
            let mut input = Input::field(key, "Paste the API key", content(0.35), cx);
            input.set_font_size(13.);
            input.set_secret(true);
            input
        });
        self._subscriptions
            .push(cx.subscribe(&input, |_, _, _: &input::Changed, cx| cx.notify()));
        self._subscriptions.push(
            cx.subscribe(&input, move |this, _, _: &input::Submitted, cx| {
                this.voice_save_key(provider, cx)
            }),
        );
        self.voice.inputs.insert(key, input.clone());
        input
    }

    /// Saves what was pasted for `provider` in the keychain and clears the
    /// field, so the key is on screen no longer than it takes to paste it.
    pub(in crate::settings) fn voice_save_key(
        &mut self,
        provider: Provider,
        cx: &mut Context<Self>,
    ) {
        let input = self.voice_input(provider, cx);
        let pasted = input.read(cx).content.to_string();
        let key = match keys::ApiKey::parse(&pasted) {
            Ok(key) => key,
            Err(error) => {
                self.voice.messages.insert(provider.id(), error.to_string());
                cx.notify();
                return;
            }
        };
        input.update(cx, |input, cx| input.set_text(String::new(), cx));
        let save = cx
            .background_executor()
            .spawn(async move { keys::save(provider, &key) });
        self.voice.jobs.push(cx.spawn(async move |this, cx| {
            let result = save.await;
            let _ = this.update(cx, |this, cx| {
                let message = match result {
                    Ok(()) => "Saved in the keychain.".to_owned(),
                    Err(error) => error.to_string(),
                };
                this.voice.messages.insert(provider.id(), message);
                this.voice_refresh(cx);
            });
        }));
        cx.notify();
    }

    pub(in crate::settings) fn voice_remove_key(
        &mut self,
        provider: Provider,
        cx: &mut Context<Self>,
    ) {
        let remove = cx
            .background_executor()
            .spawn(async move { keys::remove(provider) });
        self.voice.jobs.push(cx.spawn(async move |this, cx| {
            let result = remove.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.voice.messages.remove(provider.id()),
                    Err(error) => this.voice.messages.insert(provider.id(), error.to_string()),
                };
                if this.prefs.voice_live == provider.id() {
                    this.update(cx, |p| p.voice_live = "local".into());
                }
                this.voice_refresh(cx);
            });
        }));
    }

    /// Downloads `model`, following its progress.
    pub(in crate::settings) fn voice_download(&mut self, model: Model, cx: &mut Context<Self>) {
        if self.voice.downloading.is_some() {
            return;
        }
        self.voice.confirming = None;
        self.voice.messages.remove(model.id());
        self.voice.downloading = Some((model, 0, 0));
        let (report, incoming) = async_channel::unbounded::<Result<(u64, u64), String>>();
        let engine = crate::dictation::engine(cx);
        cx.background_executor()
            .spawn(async move {
                let mut last = 0;
                let mut step = |done: u64, total: u64| {
                    if done == total || done.saturating_sub(last) >= 1 << 20 {
                        last = done;
                        let _ = report.try_send(Ok((done, total)));
                    }
                };
                let result = match model {
                    Model::Dictation => engine.install(&mut step),
                    Model::Voice => crate::monos::group_call::model::install(step),
                };
                let _ = report.try_send(Err(result.err().unwrap_or_default()));
            })
            .detach();
        self.voice.jobs.push(cx.spawn(async move |this, cx| {
            while let Ok(update) = incoming.recv().await {
                let alive = this.update(cx, |this, cx| {
                    match update {
                        Ok((done, total)) => this.voice.downloading = Some((model, done, total)),
                        Err(error) => {
                            this.voice.downloading = None;
                            if !error.is_empty() {
                                this.voice.messages.insert(model.id(), error);
                            }
                            this.voice_refresh(cx);
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
        }));
        cx.notify();
    }

    /// The first click asks, the second deletes.
    pub(in crate::settings) fn voice_delete(&mut self, model: Model, cx: &mut Context<Self>) {
        if self.voice.confirming != Some(model) {
            self.voice.confirming = Some(model);
            cx.notify();
            return;
        }
        self.voice.confirming = None;
        let engine = crate::dictation::engine(cx);
        let remove = cx.background_executor().spawn(async move {
            match model {
                Model::Dictation => engine.remove(),
                Model::Voice => crate::monos::group_call::model::remove(),
            }
        });
        self.voice.jobs.push(cx.spawn(async move |this, cx| {
            let result = remove.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(()) => this.voice.messages.remove(model.id()),
                    Err(error) => this.voice.messages.insert(model.id(), error),
                };
                this.voice_refresh(cx);
            });
        }));
        cx.notify();
    }

    #[cfg(test)]
    pub(in crate::settings) fn voice_type_key(
        &mut self,
        provider: Provider,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let input = self.voice_input(provider, cx);
        input.update(cx, |input, cx| input.set_text(text.into(), cx));
    }

    #[cfg(test)]
    pub(in crate::settings) fn voice_field_text(
        &mut self,
        provider: Provider,
        cx: &mut Context<Self>,
    ) -> String {
        self.voice_input(provider, cx).read(cx).content.to_string()
    }

    fn model_controls(
        &self,
        model: Model,
        installed: bool,
        wants_download: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let busy = self.voice.downloading.is_some();
        let mut controls = Vec::new();
        if let Some((downloading, done, total)) = self.voice.downloading
            && downloading == model
        {
            controls.push(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.65))
                    .child(format!("Downloading {}", percent(progress(done, total))))
                    .into_any_element(),
            );
            return controls;
        }
        if wants_download {
            let download_label = if installed {
                "Get the faster model"
            } else {
                "Download"
            };
            controls.push(
                primary_button(
                    SharedString::from(format!("voice-download-{}", model.id())),
                    download_label,
                    busy,
                )
                .child(download_label)
                .on_click(cx.listener(move |this, _, _, cx| this.voice_download(model, cx)))
                .into_any_element(),
            );
        }
        if installed {
            let confirming = self.voice.confirming == Some(model);
            let delete_label = if confirming {
                "Delete from disk?"
            } else {
                "Delete"
            };
            controls.push(
                secondary_button(
                    SharedString::from(format!("voice-delete-{}", model.id())),
                    delete_label,
                    true,
                    busy,
                )
                .child(delete_label)
                .on_click(cx.listener(move |this, _, _, cx| this.voice_delete(model, cx)))
                .into_any_element(),
            );
        }
        controls
    }

    fn model_message(&self, id: &'static str) -> Option<AnyElement> {
        self.voice
            .messages
            .get(id)
            .map(|message| note(message.clone()))
    }

    pub(in crate::settings) fn voice_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.voice.checked {
            self.voice_refresh(cx);
        }
        let Some(snapshot) = self.voice.snapshot.clone() else {
            return vec![note("Reading the voice models…")];
        };
        let mut page = Vec::new();

        let status = &snapshot.dictation;
        let dictation_detail = if status.installed {
            let build = if status.neural {
                "Parakeet v3 on the Neural Engine"
            } else {
                "Parakeet v3 on the CPU"
            };
            format!("{build} · {} on disk", format_model_size(status.on_disk))
        } else {
            format!(
                "Not downloaded · {} to download",
                format_model_size(status.bytes)
            )
        };
        let upgrade = status.installed
            && !status.neural
            && monobot_dictation::Engine::preferred() == monobot_dictation::Engine::Neural;
        let mut dictation = vec![self.row(
            Some("voice-dictation"),
            true,
            "Speech model",
            Some(dictation_detail.into()),
            self.model_controls(
                Model::Dictation,
                status.installed,
                !status.installed || upgrade,
                cx,
            ),
        )];
        dictation.extend(self.model_message(Model::Dictation.id()));
        page.push(self.group(
            Some("voice-dictation-group"),
            true,
            "Dictation",
            Some("What the mic in the composer and calls hear with. It runs on this Mac; nothing is sent anywhere.".into()),
            None,
            dictation,
        ));

        let voice_detail = if snapshot.voice_installed {
            format!(
                "Supertonic 3, Spanish and English · {} on disk",
                format_model_size(snapshot.voice_on_disk)
            )
        } else {
            format!(
                "Not downloaded · {} to download",
                format_model_size(monobot_voice::model::ARCHIVE.bytes)
            )
        };
        let pace = self.prefs.voice_pace.clone();
        let pace_row = self.row(
            Some("voice-pace"),
            false,
            "Answer after a pause",
            Some("How long a call waits once you stop talking. It waits longer after «y…» or a comma, and Smart Turn tells a finished sentence from a breath.".into()),
            vec![self.segmented(
                "voice-pace",
                "Answer after a pause",
                &pace,
                &PACES,
                cx,
                |this, value, cx| this.update(cx, |p| p.voice_pace = value.into()),
            )],
        );
        let mut voice = vec![self.row(
            Some("voice-call-voice"),
            true,
            "Call voices",
            Some(voice_detail.into()),
            self.model_controls(
                Model::Voice,
                snapshot.voice_installed,
                !snapshot.voice_installed && !snapshot.voice_installing,
                cx,
            ),
        )];
        voice.extend(self.model_message(Model::Voice.id()));
        voice.push(pace_row);
        page.push(self.group(
            Some("voice-call-group"),
            false,
            "Call voices",
            Some("How group members answer out loud when the call talks on this Mac. Each member gets a voice of its own.".into()),
            None,
            voice,
        ));

        let live = self.prefs.voice_live.clone();
        let missing_key = Provider::from_id(&live)
            .is_some_and(|provider| !snapshot.keys.get(provider.id()).copied().unwrap_or(false));
        let mut rows = vec![self.row(
            Some("voice-live"),
            true,
            "Calls talk through",
            Some(if missing_key {
                "Add its API key below; until then calls talk on this Mac.".into()
            } else {
                "On this Mac, or a live model that hears and answers in real time.".into()
            }),
            vec![self.segmented(
                "voice-live",
                "Calls talk through",
                &live,
                &LIVE,
                cx,
                |this, value, cx| this.update(cx, |p| p.voice_live = value.into()),
            )],
        )];
        for provider in Provider::ALL {
            let saved = snapshot.keys.get(provider.id()).copied().unwrap_or(false);
            let input = self.voice_input(provider, cx);
            let pasted = !input.read(cx).content.trim().is_empty();
            let field = div()
                .w(px(240.))
                .h(px(32.))
                .rounded_lg()
                .border_1()
                .border_color(content(0.15))
                .bg(content(0.03))
                .px_3()
                .flex()
                .items_center()
                .child(input)
                .into_any_element();
            let mut controls = vec![
                field,
                primary_button(
                    SharedString::from(format!("voice-save-{}", provider.id())),
                    if saved { "Replace" } else { "Save" },
                    !pasted,
                )
                .child(if saved { "Replace" } else { "Save" })
                .on_click(cx.listener(move |this, _, _, cx| this.voice_save_key(provider, cx)))
                .into_any_element(),
            ];
            if saved {
                controls.push(
                    secondary_button(
                        SharedString::from(format!("voice-remove-{}", provider.id())),
                        "Remove",
                        true,
                        false,
                    )
                    .child("Remove")
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.voice_remove_key(provider, cx)),
                    )
                    .into_any_element(),
                );
            }
            let description = self
                .voice
                .messages
                .get(provider.id())
                .cloned()
                .unwrap_or_else(|| {
                    if saved {
                        "Saved in the keychain.".into()
                    } else {
                        format!("No key yet. Get one at {}", provider.key_page())
                    }
                });
            rows.push(self.row(
                Some(match provider {
                    Provider::Gemini => "voice-key-gemini",
                    Provider::Phonic => "voice-key-phonic",
                }),
                false,
                provider.label(),
                Some(description.into()),
                controls,
            ));
        }
        page.push(self.group(
            Some("voice-live-group"),
            false,
            "Live calls",
            Some("A live model hears you and answers at once, and asks your agents when it needs them. Keys stay in the system keychain.".into()),
            None,
            rows,
        ));
        page
    }
}
