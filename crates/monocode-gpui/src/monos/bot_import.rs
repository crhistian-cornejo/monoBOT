//! The dialog that imports a bots file, and the one a sync opens for the
//! bots both computers changed (BotImportDialog.tsx). New bots are added as
//! they are; one that already exists is compared first and the user keeps
//! it, replaces it or keeps both. Applying it is the shell's: this dialog
//! asks with [`Event::Apply`].
use super::bot_transfer::{Meeting, PreparedImport};
use crate::{popover::AnchorSlot, settings::widgets::primary_button, theme::*};
use gpui::{
    AnyElement, App, Context, Div, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Role,
    SharedString, Window, div, prelude::*, px,
};
use monocode_core::{
    agent::{AccessMode, Settings},
    mono::transfer::{self, Bot, Choice, Conflict, ImportPlan, Keep, Row, Verdict},
    provider::{self, Kind, Request},
};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// A provider CLI and model for one-off text (changes::TextHarness).
pub(crate) type Harness = (Kind, PathBuf, Option<String>);

pub(crate) enum Event {
    Close,
    /// The user pressed Import with these choices.
    Apply(ImportPlan),
}

/// A model's own reading of the two bots.
enum Opinion {
    Asking,
    None,
    Text(String),
}

struct Entry {
    conflict: Conflict,
    meeting: Option<Meeting>,
    opinion: Opinion,
}

pub(crate) struct BotImportDialog {
    description: String,
    from: String,
    fresh: Vec<Bot>,
    entries: Vec<Entry>,
    page: usize,
    busy: bool,
    error: Option<String>,
    max_height: f32,
    focus: FocusHandle,
    bounds: AnchorSlot,
    cancel: Arc<AtomicBool>,
}
impl EventEmitter<Event> for BotImportDialog {}
impl Focusable for BotImportDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
impl Drop for BotImportDialog {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// "Nov 5, 2026" from an RFC 3339 time (toLocaleDateString medium).
fn medium_date(exported_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(exported_at)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%b %-d, %Y")
                .to_string()
        })
        .unwrap_or_default()
}

fn accent_alpha(alpha: f32) -> gpui::Hsla {
    let mut color = accent_color();
    color.a = alpha;
    color
}

fn hex(color: &str) -> u32 {
    u32::from_str_radix(color.trim_start_matches('#'), 16).unwrap_or(0x7c3aed)
}

fn mascot(bot: &Bot) -> AnyElement {
    div()
        .size(px(16.))
        .flex_shrink_0()
        .child(super::avatar::avatar(
            &bot.mascot,
            hex(&bot.color),
            "idle",
            16.,
            true,
        ))
        .into_any_element()
}

impl BotImportDialog {
    /// `platform` is where the file came from ("windows", "macos"…) and
    /// `exported_at` its time, empty for a sync. `harness` asks a model for
    /// its reading of each pair; `None` skips it.
    pub(crate) fn new(
        platform: &str,
        exported_at: &str,
        prepared: PreparedImport,
        harness: Option<Harness>,
        cwd: PathBuf,
        max_height: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let from = transfer::platform_name(platform).to_owned();
        let total = prepared.plan.fresh.len() + prepared.plan.conflicts.len();
        let date = medium_date(exported_at);
        let description = format!(
            "{total} bot{} from {from}{}.",
            if total == 1 { "" } else { "s" },
            if date.is_empty() {
                String::new()
            } else {
                format!(", exported {date}")
            }
        );
        let cancel = Arc::new(AtomicBool::new(false));
        let mut entries = Vec::new();
        for (index, (mut conflict, meeting)) in prepared
            .plan
            .conflicts
            .into_iter()
            .zip(prepared.meetings)
            .enumerate()
        {
            // The suggestion becomes the default choice.
            if let Some(meeting) = &meeting {
                conflict.choice = transfer::default_choice(&meeting.comparison);
            }
            let opinion = match (&meeting, &harness) {
                (Some(meeting), Some(harness)) => {
                    Self::ask(
                        index,
                        meeting,
                        &conflict.incoming,
                        harness,
                        &cwd,
                        &cancel,
                        cx,
                    );
                    Opinion::Asking
                }
                _ => Opinion::None,
            };
            entries.push(Entry {
                conflict,
                meeting,
                opinion,
            });
        }
        Self {
            description,
            from,
            fresh: prepared.plan.fresh,
            entries,
            page: 0,
            busy: false,
            error: None,
            max_height,
            focus: cx.focus_handle(),
            bounds: AnchorSlot::default(),
            cancel,
        }
    }

    /// A model's short reading of the two (secondOpinion): nothing when none
    /// can be asked or it fails.
    fn ask(
        index: usize,
        meeting: &Meeting,
        incoming: &Bot,
        harness: &Harness,
        cwd: &std::path::Path,
        cancel: &Arc<AtomicBool>,
        cx: &mut Context<Self>,
    ) {
        let (kind, binary, model) = harness.clone();
        let request = Request {
            kind,
            binary,
            cwd: cwd.to_path_buf(),
            prompt: transfer::comparison_prompt(&meeting.current, incoming),
            provider_id: None,
            model,
            settings: Settings {
                access: AccessMode::ReadOnly,
                ..Default::default()
            },
        };
        let flag = cancel.clone();
        let work = cx
            .background_executor()
            .spawn(async move { provider::run_text(request, flag) });
        cx.spawn(async move |this, cx| {
            let text = work.await.ok().map(|t| t.trim().to_owned());
            let _ = this.update(cx, |this, cx| {
                if let Some(entry) = this.entries.get_mut(index) {
                    entry.opinion = match text {
                        Some(text) if !text.is_empty() => Opinion::Text(text),
                        _ => Opinion::None,
                    };
                    cx.notify();
                }
            });
        })
        .detach();
    }

    #[cfg(test)]
    pub(crate) fn choices(&self) -> Vec<Choice> {
        self.entries.iter().map(|e| e.conflict.choice).collect()
    }

    #[cfg(test)]
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub(crate) fn measured_height(&self) -> Option<f32> {
        self.bounds.get().map(|bounds| bounds.size.height.as_f32())
    }

    /// The shell could not apply the choices.
    pub(crate) fn failed(&mut self, error: String, cx: &mut Context<Self>) {
        self.busy = false;
        self.error = Some(error);
        cx.notify();
    }

    fn choose(&mut self, index: usize, choice: Choice, cx: &mut Context<Self>) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.conflict.choice = choice;
            cx.notify();
        }
    }

    fn apply(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = None;
        cx.emit(Event::Apply(ImportPlan {
            fresh: self.fresh.clone(),
            conflicts: self.entries.iter().map(|e| e.conflict.clone()).collect(),
        }));
        cx.notify();
    }

    fn ghost(&self, id: &'static str, label: &'static str, disabled: bool) -> gpui::Stateful<Div> {
        crate::modal::disabled(crate::modal::ghost_button(id, label), disabled)
    }

    fn fresh_section(&self) -> AnyElement {
        div()
            .child(
                div()
                    .mb(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.7))
                    .child("New, added as they are"),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .children(self.fresh.iter().map(|bot| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .px(px(8.))
                            .py(px(4.))
                            .child(mascot(bot))
                            .child(bot.name.clone())
                    })),
            )
            .into_any_element()
    }

    fn side_card(title: String, bot: &Bot, summary: &str) -> AnyElement {
        div()
            .flex_1()
            .min_w_0()
            .rounded_md()
            .border_1()
            .border_color(content(0.1))
            .px(px(12.))
            .py(px(8.))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .child(title),
            )
            .child(
                div()
                    .mt_1()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(mascot(bot))
                    .child(div().min_w_0().truncate().child(bot.name.clone())),
            )
            .child(
                div()
                    .mt_1()
                    .text_size(px(12.))
                    .text_color(content(0.65))
                    .child(summary.to_owned()),
            )
            .into_any_element()
    }

    fn comparison_row(row: &Row) -> AnyElement {
        let mark = |side: Verdict, cell: Div| {
            if row.verdict == side {
                cell.font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(content(1.))
            } else {
                cell.text_color(content(0.65))
            }
        };
        div()
            .flex()
            .items_start()
            .border_t_1()
            .border_color(content(0.08))
            .py(px(6.))
            .text_size(px(12.))
            .child(
                div()
                    .w(px(112.))
                    .flex_shrink_0()
                    .pr_2()
                    .text_color(content(0.5))
                    .child(row.label.clone()),
            )
            .child(mark(
                Verdict::Current,
                div().flex_1().min_w_0().pr_2().child(row.current.clone()),
            ))
            .child(
                mark(Verdict::Incoming, div().flex_1().min_w_0())
                    .child(row.incoming.clone())
                    .when(row.verdict == Verdict::Same, |d| {
                        d.child(
                            div()
                                .text_color(content(0.35))
                                .child(SharedString::from(" · same")),
                        )
                    }),
            )
            .into_any_element()
    }

    fn conflict_view(&self, entry: &Entry, meeting: &Meeting) -> AnyElement {
        let comparison = &meeting.comparison;
        let suggestion = match comparison.suggestion.keep {
            Keep::Incoming => "Suggested: the imported one. ",
            Keep::Current => "Suggested: this one. ",
            Keep::Either => "Either works. ",
        };
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(Self::side_card(
                        "On this computer".into(),
                        &meeting.current,
                        &comparison.summary.0,
                    ))
                    .child(Self::side_card(
                        format!("Imported from {}", self.from),
                        &entry.conflict.incoming,
                        &comparison.summary.1,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .children(comparison.rows.iter().map(Self::comparison_row)),
            )
            .when(!comparison.changes.is_empty(), |d| {
                d.child(
                    div()
                        .child(
                            div()
                                .mb_1()
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .child("If you replace it"),
                        )
                        .children(comparison.changes.iter().map(|change| {
                            div()
                                .flex()
                                .gap(px(6.))
                                .pl_2()
                                .text_size(px(12.))
                                .text_color(content(0.8))
                                .child("•")
                                .child(div().min_w_0().child(change.clone()))
                        })),
                )
            })
            .child(
                div()
                    .rounded_md()
                    .border_1()
                    .border_color(accent_alpha(0.25))
                    .bg(accent_alpha(0.06))
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(accent_color())
                                    .child(suggestion),
                            )
                            .child(
                                div()
                                    .text_color(content(0.75))
                                    .child(comparison.suggestion.why.clone()),
                            ),
                    )
                    .child(match &entry.opinion {
                        Opinion::Asking => div()
                            .mt(px(6.))
                            .text_color(content(0.45))
                            .child("Asking a model for its reading…"),
                        Opinion::Text(text) => div()
                            .mt(px(6.))
                            .text_color(content(0.75))
                            .child(text.clone()),
                        Opinion::None => div(),
                    }),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn choice_button(
        &self,
        index: usize,
        id: &'static str,
        label: &'static str,
        hint: &'static str,
        choice: Choice,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.entries[index].conflict.choice == choice;
        div()
            .id(id)
            .debug_selector(move || format!("bot-import-{id}"))
            .role(Role::RadioButton)
            .aria_label(label)
            .aria_selected(active)
            .flex_1()
            .min_w_0()
            .rounded_md()
            .border_1()
            .border_color(if active { accent_color() } else { content(0.1) })
            .when(active, |d| d.bg(accent_alpha(0.08)))
            .px(px(12.))
            .py(px(8.))
            .when(disabled, |d| d.opacity(0.4))
            .when(!disabled, |d| {
                d.cursor_pointer()
                    .when(!active, |d| d.hover(|s| s.bg(content(0.05))))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(index, choice, cx)))
            })
            .child(
                div()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(label),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(content(0.5))
                    .child(hint),
            )
            .into_any_element()
    }
}

impl Render for BotImportDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let busy = self.busy;
        let count = self.entries.len();
        let page = self.page.min(count.saturating_sub(1));
        let mut body = div()
            .id("bot-import-body")
            .debug_selector(|| "bot-import-body".into())
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .py(px(16.))
            .flex()
            .flex_col()
            .gap_4()
            .text_size(px(13.));
        if !self.fresh.is_empty() {
            body = body.child(self.fresh_section());
        }
        if let Some(entry) = self.entries.get(page) {
            let principal = entry.conflict.incoming.bot_role.as_deref() == Some("principal");
            let both_hint = if principal {
                "There is one orchestrator"
            } else {
                "The imported one gets a new name"
            };
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(div().font_weight(gpui::FontWeight::MEDIUM).child(format!(
                                "\u{201c}{}\u{201d} is already here",
                                entry.conflict.incoming.name
                            )))
                            .when(count > 1, |d| {
                                d.child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(content(0.5))
                                        .child(format!("{} of {count}", page + 1)),
                                )
                            }),
                    )
                    .child(match &entry.meeting {
                        Some(meeting) => self.conflict_view(entry, meeting),
                        None => div()
                            .text_color(content(0.5))
                            .child("This bot is no longer on this computer.")
                            .into_any_element(),
                    })
                    .child(
                        div()
                            .id("bot-import-choices")
                            .role(Role::RadioGroup)
                            .aria_label("What to keep")
                            .flex()
                            .gap_2()
                            .child(self.choice_button(
                                page,
                                "keep",
                                "Keep this one",
                                "The import skips it",
                                Choice::Keep,
                                false,
                                cx,
                            ))
                            .child(self.choice_button(
                                page,
                                "replace",
                                "Replace it",
                                "Its chat and folders stay",
                                Choice::Replace,
                                entry.meeting.is_none(),
                                cx,
                            ))
                            .child(self.choice_button(
                                page,
                                "both",
                                "Keep both",
                                both_hint,
                                Choice::Both,
                                principal,
                                cx,
                            )),
                    ),
            );
        }
        if let Some(error) = &self.error {
            body = body.child(
                div()
                    .id("bot-import-error")
                    .role(Role::Alert)
                    .text_size(px(12.))
                    .text_color(gpui::rgb(0xf87171))
                    .child(error.clone()),
            );
        }
        div()
            .id("bot-import-dialog")
            .key_context("BotImportDialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" && !this.busy {
                    cx.emit(Event::Close);
                    cx.stop_propagation();
                }
            }))
            .relative()
            .w_full()
            .max_h(px(self.max_height))
            .flex()
            .flex_col()
            .text_color(content(1.))
            .child(self.bounds.probe())
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .pt(px(16.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("Import bots"),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(content(0.55))
                            .child(self.description.clone()),
                    ),
            )
            .child(body)
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .border_t_1()
                    .border_color(content(0.1))
                    .px(px(20.))
                    .py(px(12.))
                    .child(div().flex().gap_1().when(count > 1, |d| {
                        d.child(
                            self.ghost("bot-import-previous", "Previous", page == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.page = this.page.saturating_sub(1);
                                    cx.notify();
                                })),
                        )
                        .child(
                            self.ghost("bot-import-next", "Next", page + 1 >= count)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.page = (this.page + 1).min(count.saturating_sub(1));
                                    cx.notify();
                                })),
                        )
                    }))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.ghost("bot-import-cancel", "Cancel", busy).on_click(
                                cx.listener(|this, _, _, cx| {
                                    if !this.busy {
                                        cx.emit(Event::Close);
                                    }
                                }),
                            ))
                            .child(
                                primary_button(
                                    "bot-import-apply",
                                    if busy { "Importing…" } else { "Import" },
                                    busy,
                                )
                                .child(if busy { "Importing…" } else { "Import" })
                                .debug_selector(|| "bot-import-apply".into())
                                .on_click(cx.listener(|this, _, _, cx| this.apply(cx))),
                            ),
                    ),
            )
    }
}
