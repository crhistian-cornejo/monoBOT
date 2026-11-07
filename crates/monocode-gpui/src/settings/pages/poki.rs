//! Poki's setup under its connector switch: its folder, whether its daemon
//! runs, the tutorial and the rules (PokiSetup.tsx, PokiTutorial.tsx and
//! pokiTutorial.ts). The work blocks, so it runs on the background executor.
use crate::{
    input::{self, Input},
    right_panel::DIFF_ADD,
    settings::{
        SettingsEvent, SettingsView,
        widgets::{RED, primary_button, secondary_button},
    },
    theme::*,
};
use gpui::{
    AnyElement, ClipboardItem, Context, FontWeight, Role, SharedString, Task, div, prelude::*, px,
    rgb,
};
use monocode_core::poki::{self, Next, Stage, Status};
use std::time::Duration;

mod scenes;

const FOLDER: &str = "poki-folder";
const REPO: &str = "https://github.com/JosssphZhou/poki";

struct Step {
    title: &'static str,
    body: &'static str,
    /// Terminal lines to copy, run in order from the Poki folder.
    commands: &'static [&'static str],
    /// Short points under the body.
    points: &'static [&'static str],
}

const STEPS: [Step; 7] = [
    Step {
        title: "What you need",
        body: "Poki is a separate open-source project (AGPL-3.0) that runs on your Mac and drives the iPhone through Xcode's developer services, over MCP. MonoBOT can install it for you and starts it while its connector is on.",
        commands: &[],
        points: &[
            "A Mac with Apple Silicon and macOS 15 or later",
            "Full Xcode, not only the command line tools",
            "Node.js 24 or later, Python 3 and ffmpeg (brew install node ffmpeg)",
            "An iPhone and its USB cable, or the iOS Simulator",
        ],
    },
    Step {
        title: "Get Poki",
        body: "Turn on Settings › Connectors › iPhone (Poki). MonoBOT asks, then downloads Poki's published release (about 11 MB), refuses it unless its SHA-256 matches the one GitHub lists, and unpacks it inside MonoBOT's own data folder. No admin rights, no install scripts, nothing else on your Mac changes.",
        commands: &[
            "git clone https://github.com/JosssphZhou/poki.git ~/poki",
            "cd ~/poki",
            "./bin/setup",
        ],
        points: &[
            "Intel Mac, or you prefer your own copy? Clone and build it yourself (needs Rust), then choose that folder:",
        ],
    },
    Step {
        title: "Connect the iPhone with its cable",
        body: "The first pairing needs the cable. Plug the iPhone into the Mac, unlock it, tap Trust and type your passcode on the phone yourself.",
        commands: &[],
        points: &[
            "After this first pairing it can also work over the same Wi-Fi, while the phone stays unlocked.",
            "Only using the Simulator? Skip to step 5.",
        ],
    },
    Step {
        title: "Turn on Developer Mode and UI Automation",
        body: "On the iPhone, open Settings › Privacy & Security › Developer Mode, turn it on and let the phone restart. Then turn on Settings › Developer › UI Automation.",
        commands: &["node bin/prepare-device.mjs"],
        points: &[
            "Run it from the Poki folder that Settings shows.",
            "No Developer Mode in Settings? Open Xcode › Window › Devices and Simulators once with the phone connected.",
            "prepare-device mounts the developer services when they are missing. It changes no security setting and downloads nothing.",
        ],
    },
    Step {
        title: "Poki runs while the connector is on",
        body: "MonoBOT starts Poki's service (its npm start) when you turn the connector on or a Phone bot needs it, and stops it when you turn it off or quit. Its address and token stay on this Mac.",
        commands: &[],
        points: &[
            "MonoBOT never starts it with POKI_MCP_APPROVAL_MODE=allow, which would approve sensitive actions without asking anyone.",
            "Running your own copy in a terminal with npm start works too; MonoBOT uses it and leaves it alone.",
        ],
    },
    Step {
        title: "Check that it says Ready",
        body: "After installing, the Poki folder fills in by itself; with your own copy, choose its folder. The status says Ready when the folder is right and Poki is running.",
        commands: &[],
        points: &[],
    },
    Step {
        title: "Make a Phone bot",
        body: "In Bots, use + › Phone, then check in its settings that iPhone (Poki) is on. It reads the screen on its own; each tap, swipe or typed text shows a card in its chat that you allow or deny.",
        commands: &[],
        points: &[
            "Keep the phone unlocked with the app in front; set Auto-Lock to a longer time while you use it.",
            "It stops and asks before sending, paying, buying, changing settings or deleting, and never enters codes or unlocks the phone.",
            "What is on the screen goes to the model you use, like any other message.",
        ],
    },
];

/// What the panel remembers between frames.
#[derive(Default)]
pub(in crate::settings) struct PokiState {
    status: Option<Status>,
    /// Whether the status was asked for since the panel last needed it.
    checked: bool,
    /// What it is doing now, when it is doing something.
    busy: Option<String>,
    error: Option<String>,
    /// The question waiting for a yes before anything downloads.
    asking: Option<String>,
    /// The tutorial's open step.
    tutorial: Option<usize>,
    copied: bool,
    generation: u64,
    check: Option<Task<()>>,
    job: Option<Task<()>>,
    progress: Option<Task<()>>,
    starting: Option<Task<()>>,
    copied_reset: Option<Task<()>>,
}

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn dot(status: Option<Status>, busy: bool) -> gpui::Hsla {
    match (busy, status) {
        (true, _) | (_, None) => accent_color(),
        (_, Some(Status::NoFolder)) => content(0.3),
        (_, Some(Status::Ready)) => rgb(DIFF_ADD).into(),
        _ => rgb(RED).into(),
    }
}

impl SettingsView {
    fn poki_folder(&self) -> String {
        self.ctx.connectors.checkout("poki").to_owned()
    }

    fn poki_draft(&self, cx: &Context<Self>) -> String {
        self.fields
            .get(FOLDER)
            .map(|input| input.read(cx).content.to_string())
            .unwrap_or_default()
    }

    fn poki_set_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(input) = self.fields.get(FOLDER) {
            input.update(cx, |input, cx| {
                if input.content != text {
                    input.set_text(text.to_owned(), cx);
                }
            });
        }
    }

    fn initialize_poki_field(&mut self, cx: &mut Context<Self>) {
        if self.fields.contains_key(FOLDER) {
            return;
        }
        let saved = self.poki_folder();
        let field = cx.new(|cx| {
            let mut field = Input::field(
                "Poki folder",
                "Installed by MonoBOT, or ~/poki",
                content(0.35),
                cx,
            );
            field.set_font_size(13.);
            field.set_text(saved, cx);
            field
        });
        self._subscriptions
            .push(cx.subscribe(&field, |_, _, _: &input::Changed, cx| cx.notify()));
        self._subscriptions
            .push(cx.subscribe(&field, |this, _, _: &input::Submitted, cx| {
                let draft = this.poki_draft(cx);
                this.poki_save(&draft, cx);
            }));
        self.fields.insert(FOLDER, field);
    }

    /// The saved folder changed (here, or by an install): show it and look again.
    pub(in crate::settings) fn poki_context_changed(
        &mut self,
        before: &str,
        cx: &mut Context<Self>,
    ) {
        let now = self.poki_folder();
        if now == before {
            return;
        }
        self.poki_set_draft(&now, cx);
        if self.poki.checked {
            self.poki_check(now, cx);
        }
    }

    /// Saves the folder with `~` spelled out: Claude runs `node <folder>/...`
    /// without a shell. Returns the saved path, or `None` when it is not one.
    fn poki_save(&mut self, path: &str, cx: &mut Context<Self>) -> Option<String> {
        if poki::folder_problem(path).is_some() {
            return None;
        }
        let full = poki::expand_home(path, &home());
        self.poki_set_draft(&full, cx);
        if full != self.poki_folder() {
            cx.emit(SettingsEvent::ConnectorCheckout(
                "poki".into(),
                full.clone(),
            ));
        }
        Some(full)
    }

    fn poki_check(&mut self, folder: String, cx: &mut Context<Self>) {
        self.poki.checked = true;
        self.poki.status = None;
        self.poki.generation += 1;
        let generation = self.poki.generation;
        self.poki.check = Some(cx.spawn(async move |this, cx| {
            let status = cx
                .background_executor()
                .spawn(async move { poki::status(&folder) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.poki.generation == generation {
                    this.poki.status = Some(status);
                    cx.notify();
                }
            });
        }));
    }

    fn poki_start(&mut self, folder: String, cx: &mut Context<Self>) {
        self.poki.busy = Some("Starting Poki…".into());
        self.poki.error = None;
        self.poki.starting = Some(cx.spawn(async move |this, cx| {
            let dir = folder.clone();
            let result = cx
                .background_executor()
                .spawn(async move { poki::start(&dir) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.poki.error = Some(error);
                }
                this.poki.busy = None;
                this.poki_check(folder, cx);
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// What turning the switch on promises: install when it is missing, then
    /// start.
    pub(in crate::settings) fn poki_bring_up(&mut self, cx: &mut Context<Self>) {
        self.poki.error = None;
        self.poki.asking = None;
        self.poki.busy = Some("Checking…".into());
        let folder = self.poki_folder();
        let data_dir = self.ctx.data_dir.clone().unwrap_or_default();
        self.poki.job = Some(cx.spawn(async move |this, cx| {
            let probe = folder.clone();
            let (status, machine) = cx
                .background_executor()
                .spawn(async move { (poki::status(&probe), poki::machine(&data_dir)) })
                .await;
            let next = poki::next(&folder, status, &machine);
            let _ = this.update(cx, |this, cx| {
                this.poki.busy = None;
                this.poki.status = Some(status);
                match next {
                    Next::Ready => {}
                    Next::Unsupported => {
                        this.poki.error = Some("Poki's package runs on Apple Silicon Macs only. The tutorial shows how to build it yourself.".into());
                    }
                    Next::Missing(missing) => {
                        this.poki.error = Some(format!(
                            "Install first, then turn it on again: {}.",
                            missing.join("; ")
                        ));
                    }
                    Next::Start(folder) => this.poki_start(folder, cx),
                    Next::Use(folder) => {
                        if let Some(full) = this.poki_save(&folder, cx) {
                            this.poki_start(full, cx);
                        }
                    }
                    Next::Install => this.poki.asking = Some(poki::install_question(&machine)),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    /// The switch went off: stop the daemon MonoBOT started.
    pub(in crate::settings) fn poki_turn_off(&mut self, cx: &mut Context<Self>) {
        self.poki = PokiState::default();
        cx.background_executor()
            .spawn(async { poki::stop() })
            .detach();
        cx.notify();
    }

    fn poki_install(&mut self, cx: &mut Context<Self>) {
        self.poki.asking = None;
        self.poki.error = None;
        self.poki.busy = Some(Stage::Download.text().into());
        let data_dir = self.ctx.data_dir.clone().unwrap_or_default();
        let (sender, stages) = async_channel::unbounded::<Stage>();
        let work = cx.background_executor().spawn(async move {
            poki::install(&data_dir, &|stage| {
                let _ = sender.try_send(stage);
            })
        });
        self.poki.progress = Some(cx.spawn(async move |this, cx| {
            while let Ok(stage) = stages.recv().await {
                let _ = this.update(cx, |this, cx| {
                    this.poki.busy = Some(stage.text().into());
                    cx.notify();
                });
            }
        }));
        self.poki.job = Some(cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |this, cx| {
                this.poki.progress = None;
                this.poki.busy = None;
                match result {
                    Ok(installed) => {
                        if let Some(full) = this.poki_save(&installed, cx) {
                            this.poki_start(full, cx);
                        }
                    }
                    Err(error) => this.poki.error = Some(error),
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn poki_choose(&mut self, cx: &mut Context<Self>) {
        let prompt = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose the Poki folder".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                this.poki_save(&path.to_string_lossy(), cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn poki_copy(&mut self, lines: &[&str], cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(lines.join("\n")));
        self.poki.copied = true;
        self.poki.copied_reset = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.poki.copied = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn poki_close_tutorial(&mut self, cx: &mut Context<Self>) {
        self.poki.tutorial = None;
        self.poki.copied = false;
        let folder = self.poki_folder();
        self.poki_check(folder, cx);
        cx.notify();
    }

    pub(in crate::settings) fn poki_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.initialize_poki_field(cx);
        if !self.poki.checked {
            let folder = self.poki_folder();
            self.poki_check(folder, cx);
        }
        let busy = self.poki.busy.clone();
        let working = busy.is_some();
        let status = self.poki.status;
        let draft = self.poki_draft(cx);
        let problem = poki::folder_problem(&draft);
        if let Some(input) = self.fields.get(FOLDER) {
            input.update(cx, |input, _| input.set_read_only(working));
        }
        let folder_field = self.fields[FOLDER].clone();

        let label = busy
            .clone()
            .or_else(|| status.map(|s| s.label().to_owned()))
            .unwrap_or_else(|| "Checking…".into());
        let hint = (!working).then(|| status.map(Status::hint)).flatten();

        let action = match status {
            Some(Status::Stopped) if !working => {
                secondary_button("poki-start", "Start Poki", false, false).on_click(cx.listener(
                    move |this, _, _, cx| {
                        let draft = this.poki_draft(cx);
                        let folder = this
                            .poki_save(&draft, cx)
                            .unwrap_or_else(|| this.poki_folder());
                        this.poki_start(folder, cx);
                    },
                ))
            }
            Some(Status::NoFolder) if !working => {
                secondary_button("poki-install", "Install Poki", false, false)
                    .on_click(cx.listener(|this, _, _, cx| this.poki_bring_up(cx)))
            }
            _ => {
                secondary_button("poki-check", "Check again", false, working).when(!working, |d| {
                    d.on_click(cx.listener(|this, _, _, cx| {
                        let draft = this.poki_draft(cx);
                        let folder = this
                            .poki_save(&draft, cx)
                            .unwrap_or_else(|| this.poki_folder());
                        this.poki_check(folder, cx);
                        cx.notify();
                    }))
                })
            }
        };

        div()
            .id("poki-setup")
            .debug_selector(|| "settings-poki-setup".into())
            .border_t_1()
            .border_color(content(0.1))
            .bg(content(0.02))
            .px_3()
            .py_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(content(0.7))
                            .child("Poki folder"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .h(px(36.))
                                    .min_w_0()
                                    .flex_1()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .bg(background_base())
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .when(working, |d| d.opacity(0.6))
                                    .child(folder_field),
                            )
                            .child(
                                secondary_button("poki-choose", "Choose…", false, working).when(
                                    !working,
                                    |d| d.on_click(cx.listener(|this, _, _, cx| this.poki_choose(cx))),
                                ),
                            ),
                    )
                    .when_some(problem, |d, problem| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(RED))
                                .child(problem),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .size(px(8.))
                                    .flex_shrink_0()
                                    .rounded_full()
                                    .bg(dot(status, working)),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(label),
                                    )
                                    .when_some(hint, |d, hint| {
                                        d.child(
                                            div()
                                                .text_size(px(12.))
                                                .line_height(px(16.))
                                                .text_color(content(0.55))
                                                .child(hint),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(action)
                            .child(
                                primary_button("poki-tutorial", "How to set it up", false)
                                    .debug_selector(|| "settings-poki-tutorial-open".into())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.poki.tutorial = Some(0);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .when_some(self.poki.error.clone(), |d, error| {
                d.child(
                    div()
                        .border_1()
                        .border_color(content(0.1))
                        .px(px(10.))
                        .py_2()
                        .text_size(px(12.))
                        .line_height(px(16.))
                        .text_color(rgb(RED))
                        .child(error),
                )
            })
            .when_some(self.poki.asking.clone(), |d, question| {
                d.child(self.poki_question(question, cx))
            })
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(16.))
                    .text_color(content(0.55))
                    .child("It acts on your real phone. Only a bot you give it to can use it; every tap, swipe and keystroke asks you first. It never reads or enters codes, never unlocks the phone and never accepts a purchase. Screens are sent to the model you use. MonoBOT installs only the published Poki release, checked against its SHA-256, and never starts it with POKI_MCP_APPROVAL_MODE=allow."),
            )
            .when_some(self.poki.tutorial, |d, step| {
                d.child(self.poki_tutorial(step, status, cx))
            })
            .into_any_element()
    }

    /// The question before anything downloads (the "Install Poki" dialog).
    fn poki_question(&mut self, question: String, cx: &mut Context<Self>) -> AnyElement {
        div()
            .border_1()
            .border_color(content(0.1))
            .bg(background_base())
            .p_3()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Install Poki"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(content(0.7))
                    .child(question),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        secondary_button("poki-install-no", "Not now", false, false).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.poki.asking = None;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        primary_button("poki-install-yes", "Download and install", false)
                            .on_click(cx.listener(|this, _, _, cx| this.poki_install(cx))),
                    ),
            )
            .into_any_element()
    }

    /// PokiTutorial: the seven steps, one at a time.
    fn poki_tutorial(
        &mut self,
        at: usize,
        status: Option<Status>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let at = at.min(STEPS.len() - 1);
        let step = &STEPS[at];
        let last = at == STEPS.len() - 1;
        let copied = self.poki.copied;
        let list = STEPS.iter().enumerate().map(|(index, entry)| {
            let current = index == at;
            div()
                .id(SharedString::from(format!("poki-step-{index}")))
                .role(Role::Button)
                .aria_label(entry.title)
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .py(px(6.))
                .text_size(px(12.))
                .cursor_pointer()
                .when(current, |d| d.bg(content(0.1)).text_color(content(1.)))
                .when(!current, |d| {
                    d.text_color(content(0.6)).hover(|s| s.bg(content(0.08)))
                })
                .child(
                    div()
                        .size(px(18.))
                        .flex_shrink_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(if current || index < at {
                            accent_color()
                        } else {
                            content(0.2)
                        })
                        .text_size(px(10.))
                        .text_color(if current {
                            accent_color()
                        } else {
                            content(0.6)
                        })
                        .child(if index < at {
                            "✓".to_owned()
                        } else {
                            (index + 1).to_string()
                        }),
                )
                .child(div().truncate().child(entry.title))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.poki.tutorial = Some(index);
                    cx.notify();
                }))
        });
        let right_now = status.map_or("checking…", Status::label);
        div()
            .id("poki-tutorial")
            .debug_selector(|| "settings-poki-tutorial".into())
            .border_1()
            .border_color(content(0.1))
            .bg(background_base())
            .flex()
            .child(
                div()
                    .w(px(190.))
                    .flex_shrink_0()
                    .border_r_1()
                    .border_color(content(0.1))
                    .p_2()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(list),
            )
            .child(
                div()
                    .min_w_0()
                    .flex_1()
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(scenes::view(at))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(format!("STEP {} OF {}", at + 1, STEPS.len())),
                            )
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(step.title),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .line_height(px(21.))
                                    .text_color(content(0.75))
                                    .child(step.body),
                            ),
                    )
                    .when(!step.commands.is_empty(), |d| {
                        let lines = step.commands;
                        d.child(
                            div()
                                .flex()
                                .items_start()
                                .justify_between()
                                .gap_3()
                                .border_1()
                                .border_color(content(0.1))
                                .px_3()
                                .py_2()
                                .child(
                                    div()
                                        .min_w_0()
                                        .font_family("Menlo")
                                        .text_size(px(12.))
                                        .children(lines.iter().map(|line| {
                                            div()
                                                .flex()
                                                .child(div().text_color(content(0.35)).child("$ "))
                                                .child(*line)
                                        })),
                                )
                                .child(
                                    secondary_button(
                                        "poki-copy",
                                        if copied { "Copied" } else { "Copy" },
                                        false,
                                        false,
                                    )
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.poki_copy(lines, cx)
                                        }),
                                    ),
                                ),
                        )
                    })
                    .when(at == 5, |d| {
                        d.child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.6))
                                .child(format!("Right now: {right_now}")),
                        )
                    })
                    .when(!step.points.is_empty(), |d| {
                        d.child(div().flex().flex_col().gap(px(4.)).children(
                            step.points.iter().map(|point| {
                                div()
                                    .flex()
                                    .gap_2()
                                    .text_size(px(12.))
                                    .line_height(px(16.))
                                    .text_color(content(0.6))
                                    .child(
                                        div()
                                            .mt(px(6.))
                                            .size(px(4.))
                                            .flex_shrink_0()
                                            .bg(content(0.4)),
                                    )
                                    .child(div().min_w_0().child(*point))
                            }),
                        ))
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_3()
                            .border_t_1()
                            .border_color(content(0.1))
                            .pt_3()
                            .child(
                                div()
                                    .id("poki-repo")
                                    .role(Role::Link)
                                    .aria_label("Poki on GitHub")
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .cursor_pointer()
                                    .hover(|s| s.text_color(content(1.)).underline())
                                    .child("Poki on GitHub (AGPL-3.0)")
                                    .on_click(|_, _, cx| cx.open_url(REPO)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_shrink_0()
                                    .items_center()
                                    .gap_2()
                                    .child(
                                        secondary_button("poki-back", "Back", false, at == 0).when(
                                            at > 0,
                                            |d| {
                                                d.on_click(cx.listener(move |this, _, _, cx| {
                                                    this.poki.tutorial = Some(at - 1);
                                                    cx.notify();
                                                }))
                                            },
                                        ),
                                    )
                                    .child(
                                        primary_button(
                                            "poki-next",
                                            if last { "Done" } else { "Next" },
                                            false,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                if last {
                                                    this.poki_close_tutorial(cx);
                                                } else {
                                                    this.poki.tutorial = Some(at + 1);
                                                    cx.notify();
                                                }
                                            }),
                                        ),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}
