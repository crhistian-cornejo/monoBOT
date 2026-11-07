//! GroupDialog: names a group, says what it is for and picks who is in it;
//! mirrors GroupDialog.tsx from the Tauri desktop app.
use super::*;
use crate::{
    input::{Changed as InputChanged, Input},
    modal,
    theme::*,
};
use gpui::{
    AnyElement, App, Context, Entity, EventEmitter, FocusHandle, Focusable, KeyDownEvent, Role,
    SharedString, Subscription, Window, div, prelude::*, px, rgb,
};
use monocode_core::mono::{
    Mono, Roster, bot_tools,
    groups::{
        Folder, Group,
        brief::member_role,
        group::{FOLDERS_MAX, GROUP_CONTEXT_MAX, GROUP_MAX_MEMBERS},
        native,
        project::{GroupDraft, group_draft, seed_group_memory},
    },
    memory,
};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub(crate) struct GroupCreate {
    pub name: String,
    pub goal: String,
    pub members: Vec<String>,
    pub folders: Vec<Folder>,
    pub context: Vec<String>,
    pub instructions: Option<String>,
    pub memory: Option<String>,
    pub add_task: bool,
}

#[derive(Clone, Debug)]
pub(crate) enum Event {
    Close,
    Create(GroupCreate),
    Save(String, GroupDraft),
    OpenPage(String, &'static str),
}

pub(crate) struct GroupDialog {
    initial_id: Option<String>,
    roster: Roster,
    name: Entity<Input>,
    goal: Entity<Input>,
    instructions: Entity<Input>,
    memory: Entity<Input>,
    members: Vec<String>,
    folders: Vec<Folder>,
    context: Vec<String>,
    add_task: bool,
    sections_open: HashSet<&'static str>,
    problem: Option<String>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<Event> for GroupDialog {}
impl Focusable for GroupDialog {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl GroupDialog {
    pub(crate) fn new(
        initial: Option<(String, GroupDraft)>,
        roster: Roster,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (initial_id, initial_draft) = match initial {
            Some((id, draft)) => (Some(id), Some(draft)),
            None => (None, None),
        };

        let initial_name = initial_draft
            .as_ref()
            .map(|d| d.name.clone())
            .unwrap_or_default();
        let initial_goal = initial_draft
            .as_ref()
            .map(|d| d.goal.clone())
            .unwrap_or_default();
        let initial_folders = initial_draft
            .as_ref()
            .map(|d| d.folders.clone())
            .unwrap_or_default();
        let initial_context = initial_draft
            .as_ref()
            .map(|d| d.context.clone())
            .unwrap_or_default();

        let mut members = initial_draft
            .as_ref()
            .map(|d| d.members.clone())
            .unwrap_or_default();
        if let Some(principal) = roster.monos.iter().find(|m| m.is_principal())
            && !members.contains(&principal.id)
        {
            members.insert(0, principal.id.clone());
        }

        let mut sections_open = HashSet::new();
        if !initial_folders.is_empty() {
            sections_open.insert("folders");
        }
        if !initial_context.is_empty() {
            sections_open.insert("context");
        }

        let name = cx.new(|cx| {
            let mut f = Input::field(
                "Group name",
                "Morning briefing, Trip planning, Report review…",
                content(0.35),
                cx,
            );
            f.set_text(initial_name, cx);
            f
        });

        let goal = cx.new(|cx| {
            let mut f = Input::multiline_field(
                "What it is for",
                "Shared with every member when the group brings them in.",
                cx,
            );
            f.set_text(initial_goal, cx);
            f
        });

        let instructions = cx.new(|cx| {
            Input::multiline_field(
                "Instructions",
                "Answer in Spanish. Review the drawings before the reports…",
                cx,
            )
        });

        let memory = cx.new(|cx| {
            Input::multiline_field(
                "Memory",
                "One fact per line: the client, the deadline, where things live…",
                cx,
            )
        });

        let subscriptions = vec![cx.subscribe(&name, |_, _, _: &InputChanged, cx| {
            cx.notify();
        })];

        Self {
            initial_id,
            roster,
            name,
            goal,
            instructions,
            memory,
            members,
            folders: initial_folders,
            context: initial_context,
            add_task: false,
            sections_open,
            problem: None,
            focus: cx.focus_handle(),
            _subscriptions: subscriptions,
        }
    }

    fn editing(&self) -> bool {
        self.initial_id.is_some()
    }

    fn ready(&self, cx: &App) -> bool {
        !self.name.read(cx).content.trim().is_empty()
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let name = self.name.read(cx).content.trim().to_string();
        if name.is_empty() {
            return;
        }
        let goal = self.goal.read(cx).content.trim().to_string();
        if let Some(id) = self.initial_id.clone() {
            cx.emit(Event::Save(
                id,
                GroupDraft {
                    name,
                    goal,
                    members: self.members.clone(),
                    folders: self.folders.clone(),
                    context: self.context.clone(),
                },
            ));
        } else {
            let instructions = {
                let t = self.instructions.read(cx).content.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            };
            let memory = {
                let t = self.memory.read(cx).content.trim().to_string();
                if t.is_empty() { None } else { Some(t) }
            };
            cx.emit(Event::Create(GroupCreate {
                name,
                goal,
                members: self.members.clone(),
                folders: self.folders.clone(),
                context: self.context.clone(),
                instructions,
                memory,
                add_task: self.add_task,
            }));
        }
    }

    fn pick_folders(&mut self, cx: &mut Context<Self>) {
        self.problem = None;
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open folders to this group".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let mut next = this.folders.clone();
                let mut problem = None;
                for path in paths {
                    let path_str = path.to_string_lossy().into_owned();
                    if let Some(why) = bot_tools::folder_problem(&path_str, false) {
                        problem = Some(format!("{path_str}: {why}"));
                        continue;
                    }
                    if !next.iter().any(|f| f.path == path_str) {
                        next.push(Folder::new(&path_str, false));
                    }
                }
                if next.len() > FOLDERS_MAX {
                    problem = Some(format!("At most {FOLDERS_MAX} folders"));
                    next.truncate(FOLDERS_MAX);
                }
                this.problem = problem;
                this.folders = next;
                this.sections_open.insert("folders");
                cx.notify();
            });
        })
        .detach();
    }

    fn pick_context_files(&mut self, cx: &mut Context<Self>) {
        self.problem = None;
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add context files".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let mut next = this.context.clone();
                let mut problem = None;
                for path in paths {
                    let path_str = path.to_string_lossy().into_owned();
                    if let Some(why) = bot_tools::folder_problem(&path_str, false) {
                        problem = Some(format!("{path_str}: {why}"));
                        continue;
                    }
                    if !next.contains(&path_str) {
                        next.push(path_str);
                    }
                }
                if next.len() > GROUP_CONTEXT_MAX {
                    problem = Some(format!("At most {GROUP_CONTEXT_MAX} files"));
                    next.truncate(GROUP_CONTEXT_MAX);
                }
                this.problem = problem;
                this.context = next;
                this.sections_open.insert("context");
                cx.notify();
            });
        })
        .detach();
    }
}

fn pretty_file_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}

fn check_icon(checked: bool) -> gpui::Div {
    div()
        .size(px(16.))
        .rounded(px(4.))
        .border_1()
        .border_color(if checked {
            accent_color()
        } else {
            content(0.25)
        })
        .bg(if checked {
            accent_color()
        } else {
            gpui::transparent_black()
        })
        .flex()
        .items_center()
        .justify_center()
        .when(checked, |d| {
            d.child(icon("check-2", 12.).text_color(gpui::white()))
        })
}

impl gpui::Render for GroupDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editing = self.editing();
        let ready = self.ready(cx);
        let id = self.initial_id.clone();

        let candidates: Vec<Mono> = {
            let mut list: Vec<Mono> = self
                .roster
                .monos
                .iter()
                .filter(|m| !m.is_group())
                .cloned()
                .collect();
            list.sort_by_key(|m| !m.is_principal());
            list
        };

        let field_box = || {
            div()
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base())
                .px_2p5()
                .py_1p5()
                .text_size(px(13.))
        };

        div()
            .id("group-dialog")
            .debug_selector(|| "group-dialog".into())
            .key_context("GroupDialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Event::Close);
                    cx.stop_propagation();
                }
            }))
            .relative()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .text_color(content(1.))
            // Header
            .child(
                div()
                    .flex_shrink_0()
                    .px(px(20.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(if editing {
                                "Group settings"
                            } else {
                                "New group"
                            }),
                    )
                    .child(
                        div()
                            .mt_1()
                            .text_size(px(12.))
                            .text_color(content(0.55))
                            .child(if editing {
                                "Its goal, members and what they may open"
                            } else {
                                "Agents that work on one goal together, led by your Principal"
                            }),
                    ),
            )
            // Scrollable Form Body
            .child(
                div()
                    .id("group-dialog-body")
                    .debug_selector(|| "group-dialog-body".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(20.))
                    .pb(px(16.))
                    .flex()
                    .flex_col()
                    .gap_3()
                    // Name field
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.7))
                                    .child("Name"),
                            )
                            .child(field_box().child(self.name.clone())),
                    )
                    // Goal field
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.7))
                                    .child("What it is for"),
                            )
                            .child(field_box().child(self.goal.clone())),
                    )
                    // Members
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(content(0.7))
                                            .child("Members"),
                                    )
                                    .child(
                                        div().text_size(px(12.)).text_color(content(0.35)).child(
                                            format!("{}/{}", self.members.len(), GROUP_MAX_MEMBERS),
                                        ),
                                    ),
                            )
                            .child(
                                div()
                                    .id("group-dialog-members")
                                    .max_h(px(160.))
                                    .overflow_y_scroll()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .bg(background_base())
                                    .flex()
                                    .flex_col()
                                    .children(candidates.into_iter().map(|candidate| {
                                        let is_lead = candidate.is_principal();
                                        let cid = candidate.id.clone();
                                        let on = self.members.contains(&cid);
                                        let full = self.members.len() >= GROUP_MAX_MEMBERS;
                                        let specialty =
                                            candidate.specialty.clone().unwrap_or_else(|| {
                                                if is_lead {
                                                    "Orchestrator".into()
                                                } else {
                                                    candidate.label()
                                                }
                                            });

                                        if is_lead {
                                            div()
                                                .id(SharedString::from(format!(
                                                    "lead-{}",
                                                    candidate.id
                                                )))
                                                .h(px(32.))
                                                .px_2()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .child(avatar::avatar(
                                                    &candidate.mascot,
                                                    candidate.color,
                                                    "idle",
                                                    16.,
                                                    true,
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(13.))
                                                                .font_weight(
                                                                    gpui::FontWeight::MEDIUM,
                                                                )
                                                                .child(candidate.label()),
                                                        )
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(11.))
                                                                .text_color(content(0.5))
                                                                .child(specialty),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .px_1p5()
                                                        .py_0p5()
                                                        .rounded_md()
                                                        .bg(accent_color().opacity(0.1))
                                                        .text_color(accent_color())
                                                        .text_size(px(11.))
                                                        .font_weight(gpui::FontWeight::MEDIUM)
                                                        .child("Leads"),
                                                )
                                                .into_any_element()
                                        } else {
                                            div()
                                                .id(SharedString::from(format!(
                                                    "member-row-{}",
                                                    candidate.id
                                                )))
                                                .h(px(32.))
                                                .px_2()
                                                .flex()
                                                .items_center()
                                                .gap_2()
                                                .cursor_pointer()
                                                .hover(|s| s.bg(content(0.04)))
                                                .child(avatar::avatar(
                                                    &candidate.mascot,
                                                    candidate.color,
                                                    "idle",
                                                    16.,
                                                    true,
                                                ))
                                                .child(
                                                    div()
                                                        .flex_1()
                                                        .min_w_0()
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(13.))
                                                                .font_weight(
                                                                    gpui::FontWeight::MEDIUM,
                                                                )
                                                                .child(candidate.label()),
                                                        )
                                                        .child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(11.))
                                                                .text_color(content(0.5))
                                                                .child(specialty),
                                                        ),
                                                )
                                                .child(check_icon(on))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if this.members.contains(&cid) {
                                                        this.members.retain(|x| x != &cid);
                                                    } else if !full {
                                                        this.members.push(cid.clone());
                                                    }
                                                    cx.notify();
                                                }))
                                                .into_any_element()
                                        }
                                    })),
                            ),
                    )
                    // Project · optional
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_1()
                                    .text_size(px(12.))
                                    .text_color(content(0.7))
                                    .child("Project")
                                    .child(div().text_color(content(0.4)).child("· optional")),
                            )
                            .child(
                                div()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(content(0.1))
                                    .bg(background_base())
                                    .flex()
                                    .flex_col()
                                    // Row 1: Folders
                                    .child(self.render_folders_row(cx))
                                    .child(div().h(px(1.)).bg(content(0.06)))
                                    // Row 2: Instructions
                                    .child(self.render_instructions_row(editing, &id, cx))
                                    .child(div().h(px(1.)).bg(content(0.06)))
                                    // Row 3: Context
                                    .child(self.render_context_row(cx))
                                    .child(div().h(px(1.)).bg(content(0.06)))
                                    // Row 4: Memory
                                    .child(self.render_memory_row(editing, &id, cx))
                                    .child(div().h(px(1.)).bg(content(0.06)))
                                    // Row 5: Scheduled tasks
                                    .child(self.render_habits_row(editing, &id, cx)),
                            ),
                    )
                    // Problem alert
                    .when_some(self.problem.as_ref(), |d, prob| {
                        d.child(
                            div()
                                .id("group-dialog-problem-alert")
                                .role(Role::Alert)
                                .text_size(px(12.))
                                .text_color(rgb(0xf87171))
                                .child(prob.clone()),
                        )
                    }),
            )
            // Footer
            .child(
                div()
                    .debug_selector(|| "group-dialog-footer".into())
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .border_t_1()
                    .border_color(content(0.1))
                    .px(px(20.))
                    .py(px(12.))
                    .child(
                        modal::ghost_button("group-dialog-cancel", "Cancel").on_click(cx.listener(
                            |_, _, _, cx| {
                                cx.emit(Event::Close);
                            },
                        )),
                    )
                    .child(
                        modal::disabled(
                            modal::primary_button(
                                "group-dialog-submit",
                                if editing { "Save" } else { "Create group" },
                            ),
                            !ready,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.submit(cx);
                        })),
                    ),
            )
    }
}

impl GroupDialog {
    fn render_folders_row(&self, cx: &Context<Self>) -> AnyElement {
        let open = self.sections_open.contains("folders");
        let count = self.folders.len();
        let note = if count > 0 {
            format!("{count} open to the members")
        } else {
            "Open folders to the members, read only or to change".to_string()
        };

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(40.))
                    .px_3()
                    .py_1p5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("folder-01", 16.).text_color(content(0.55)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(content(0.9))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Folders"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(note),
                            ),
                    )
                    .when(!open, |d| {
                        d.child(
                            div()
                                .id("folders-add-btn")
                                .cursor_pointer()
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .flex()
                                .items_center()
                                .gap_1()
                                .text_size(px(12.))
                                .text_color(content(0.65))
                                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                .child(icon("plus", 12.))
                                .child("Add")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.sections_open.insert("folders");
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .when(open, |d| {
                d.child(
                    div()
                        .px_3()
                        .pb_2()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .children(self.folders.iter().enumerate().map(|(ix, folder)| {
                            let path = folder.path.clone();
                            let edit = folder.edit;
                            div()
                                .id(SharedString::from(format!("folder-entry-{ix}")))
                                .h(px(28.))
                                .px_2()
                                .rounded_md()
                                .bg(content(0.03))
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(icon("folder-01", 14.).text_color(content(0.5)))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(content(0.85))
                                        .child(pretty_file_path(&path)),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("folder-edit-{ix}")))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .cursor_pointer()
                                        .child(check_icon(edit))
                                        .child(
                                            div()
                                                .text_size(px(11.))
                                                .text_color(content(0.6))
                                                .child("Can change"),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if let Some(f) = this.folders.get_mut(ix) {
                                                f.edit = !edit;
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id(SharedString::from(format!("folder-del-{ix}")))
                                        .cursor_pointer()
                                        .size(px(20.))
                                        .rounded_md()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .hover(|s| s.bg(content(0.08)))
                                        .child(icon("close", 12.).text_color(content(0.45)))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.folders.retain(|f| f.path != path);
                                            cx.notify();
                                        })),
                                )
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .pt_1()
                                .child(
                                    div()
                                        .id("open-folders-btn")
                                        .cursor_pointer()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_size(px(12.))
                                        .text_color(content(0.7))
                                        .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                        .child(icon("plus", 12.))
                                        .child("Open folders")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.pick_folders(cx);
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .line_height(px(16.))
                                .text_color(content(0.45))
                                .child("Members read these without asking. Tick “Can change” to let them edit there; a bot edits only if its own tools allow changing files, and nobody deletes. For anything else, your Principal asks you first."),
                        ),
                )
            })
            .into_any_element()
    }

    fn render_instructions_row(
        &self,
        editing: bool,
        id: &Option<String>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let open = self.sections_open.contains("instructions");
        let id_val = id.clone();

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(40.))
                    .px_3()
                    .py_1p5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("sparkles", 16.).text_color(content(0.55)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(content(0.9))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Instructions"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child("How the group works: tone, steps, what to check"),
                            ),
                    )
                    .child(if editing {
                        div()
                            .id("instructions-edit-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child("Edit")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if let Some(id) = id_val.clone() {
                                    cx.emit(Event::OpenPage(id, "instructions"));
                                }
                            }))
                            .into_any_element()
                    } else if !open {
                        div()
                            .id("instructions-add-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child(icon("plus", 12.))
                            .child("Add")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sections_open.insert("instructions");
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .when(!editing && open, |d| {
                d.child(
                    div().px_3().pb_3().child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(background_base())
                            .px_2p5()
                            .py_1p5()
                            .child(self.instructions.clone()),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_context_row(&self, cx: &Context<Self>) -> AnyElement {
        let open = self.sections_open.contains("context");
        let count = self.context.len();
        let note = if count > 0 {
            format!(
                "{count} file{} the members read",
                if count == 1 { "" } else { "s" }
            )
        } else {
            "Files the members read for background".to_string()
        };

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(40.))
                    .px_3()
                    .py_1p5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("file", 16.).text_color(content(0.55)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(content(0.9))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Context"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child(note),
                            ),
                    )
                    .child(
                        div()
                            .id("context-add-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child(icon("plus", 12.))
                            .child("Add")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.pick_context_files(cx);
                            })),
                    ),
            )
            .when(open && count > 0, |d| {
                d.child(div().px_3().pb_2().flex().flex_col().gap_1().children(
                    self.context.iter().enumerate().map(|(ix, path)| {
                        let path_val = path.clone();
                        div()
                            .id(SharedString::from(format!("context-file-{ix}")))
                            .h(px(26.))
                            .px_2()
                            .rounded_md()
                            .bg(content(0.03))
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(icon("file", 13.).text_color(content(0.5)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(content(0.85))
                                    .child(pretty_file_path(path)),
                            )
                            .child(
                                div()
                                    .id(SharedString::from(format!("context-del-{ix}")))
                                    .cursor_pointer()
                                    .size(px(20.))
                                    .rounded_md()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .hover(|s| s.bg(content(0.08)))
                                    .child(icon("close", 12.).text_color(content(0.45)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.context.retain(|p| p != &path_val);
                                        cx.notify();
                                    })),
                            )
                    }),
                ))
            })
            .into_any_element()
    }

    fn render_memory_row(
        &self,
        editing: bool,
        id: &Option<String>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let open = self.sections_open.contains("memory");
        let id_val = id.clone();

        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .min_h(px(40.))
                    .px_3()
                    .py_1p5()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(icon("sticky-note", 16.).text_color(content(0.55)))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(content(0.9))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Memory"),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(content(0.45))
                                    .child("What the group remembers between conversations"),
                            ),
                    )
                    .child(if editing {
                        div()
                            .id("memory-see-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child("See")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                if let Some(id) = id_val.clone() {
                                    cx.emit(Event::OpenPage(id, "memory"));
                                }
                            }))
                            .into_any_element()
                    } else if !open {
                        div()
                            .id("memory-add-btn")
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(12.))
                            .text_color(content(0.65))
                            .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                            .child(icon("plus", 12.))
                            .child("Add")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sections_open.insert("memory");
                                cx.notify();
                            }))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
            .when(!editing && open, |d| {
                d.child(
                    div().px_3().pb_3().child(
                        div()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(background_base())
                            .px_2p5()
                            .py_1p5()
                            .child(self.memory.clone()),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_habits_row(
        &self,
        editing: bool,
        id: &Option<String>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id_val = id.clone();
        let add_task = self.add_task;
        let note = if editing {
            "Habits the group runs on its own"
        } else if add_task {
            "Its Habits open on a new task once it is created"
        } else {
            "Habits the group runs on its own, on a schedule"
        };

        div()
            .min_h(px(40.))
            .px_3()
            .py_1p5()
            .flex()
            .items_center()
            .gap_3()
            .child(icon("clock", 16.).text_color(content(0.55)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(content(0.9))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child("Scheduled tasks"),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(content(0.45))
                            .child(note),
                    ),
            )
            .child(if editing {
                div()
                    .id("habits-open-btn")
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_size(px(12.))
                    .text_color(content(0.65))
                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    .child("Open")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        if let Some(id) = id_val.clone() {
                            cx.emit(Event::OpenPage(id, "habits"));
                        }
                    }))
                    .into_any_element()
            } else {
                div()
                    .id("habits-toggle-btn")
                    .debug_selector(|| {
                        if add_task {
                            "group-dialog-habits-undo"
                        } else {
                            "group-dialog-habits-toggle"
                        }
                        .into()
                    })
                    .cursor_pointer()
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .text_size(px(12.))
                    .text_color(content(0.65))
                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    .child(if add_task { "Undo" } else { "Add" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.add_task = !this.add_task;
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }
}

impl Monos {
    /// The group editor over the whole surface (App.tsx's `groupDialogOpen`
    /// and `groupSettingsFor`): `edit` names the group to change, `None`
    /// starts a new one. One dialog at a time.
    pub(super) fn open_group_dialog(
        &mut self,
        edit: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.group_dialog.is_some() {
            return;
        }
        let initial = edit.and_then(|id| {
            let mono = self.roster.get(&id)?;
            let group = mono.group.as_ref()?;
            Some((id, group_draft(&mono.label(), group)))
        });
        let roster = self.roster.clone();
        let view = cx.new(|cx| GroupDialog::new(initial, roster, window, cx));
        let events = cx.subscribe_in(
            &view,
            window,
            |this, _, event: &Event, window, cx| match event {
                Event::Create(draft) => this.create_group(draft, window, cx),
                Event::Save(id, draft) => {
                    this.update_group(id, draft, cx);
                    this.close_group_dialog(cx);
                }
                Event::OpenPage(id, page) => {
                    let (id, page) = (id.clone(), *page);
                    this.close_group_dialog(cx);
                    this.open(&id, window, cx);
                    match page {
                        "instructions" => {
                            this.open_current_settings_section(SettingsSection::Soul, window, cx)
                        }
                        "memory" => {
                            this.open_current_settings_section(SettingsSection::Memory, window, cx)
                        }
                        "habits" => {
                            this.overview_tab = OverviewTab::Habits;
                            this.show_overview(cx);
                        }
                        _ => {}
                    }
                }
                Event::Close => this.close_group_dialog(cx),
            },
        );
        window.focus(&view.focus_handle(cx), cx);
        self.group_dialog = Some((view, events, window.viewport_size()));
        cx.notify();
    }

    fn close_group_dialog(&mut self, cx: &mut Context<Self>) {
        if self.group_dialog.take().is_some() {
            cx.notify();
        }
    }

    /// App.tsx's `onStartGroup`: the group itself, its first memories, and
    /// its chat — or its Habits on a new scheduled task when asked.
    fn create_group(&mut self, draft: &GroupCreate, window: &mut Window, cx: &mut Context<Self>) {
        let group = Group {
            members: draft.members.clone(),
            goal: (!draft.goal.is_empty()).then(|| draft.goal.clone()),
            folders: draft.folders.clone(),
            context: draft.context.clone(),
            ..Group::default()
        };
        let id = self
            .roster
            .create_group(
                &self.root,
                &draft.name,
                group,
                draft.instructions.as_deref(),
            )
            .id
            .clone();
        if let Some(memory) = &draft.memory {
            seed_group_memory(&self.root, &id, memory, &memory::date());
        }
        self.close_group_dialog(cx);
        self.open(&id, window, cx);
        // Its Members (or a new scheduled task on its Habits), like the
        // desktop app lands on after creating a group.
        self.overview_tab = if draft.add_task {
            self.route = Some(Route::NewHabit);
            OverviewTab::Habits
        } else {
            OverviewTab::Members
        };
        self.show_overview(cx);
    }

    /// App.tsx's `onSaveGroup`: the draft's fields, keeping the answers,
    /// requests and seats the group collected meanwhile.
    fn update_group(&mut self, id: &str, draft: &GroupDraft, cx: &mut Context<Self>) {
        let mut group = self
            .roster
            .get(id)
            .and_then(|mono| mono.group.clone())
            .unwrap_or_default();
        group.members = draft.members.clone();
        group.goal = (!draft.goal.is_empty()).then(|| draft.goal.clone());
        group.folders = draft.folders.clone();
        group.context = draft.context.clone();
        self.roster.save_group(id, &draft.name, group);
        self.configure_chat(id, cx);
        self.save(cx);
    }

    /// The open group editor, when there is one: a scrim under the centred
    /// card, like the Mono settings modal. Rendered on both surfaces, since
    /// the dialog also opens from the rail with no Mono selected.
    pub(super) fn group_dialog_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (view, _, size) = self.group_dialog.as_ref()?;
        let width = 560f32.min(size.width.as_f32() - 24.);
        let height = 680f32.min(size.height.as_f32() - 32.);
        let weak = cx.weak_entity();
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    gpui::deferred(
                        gpui::anchored()
                            .position(gpui::point(px(0.), px(0.)))
                            .child(
                                div()
                                    .w(size.width)
                                    .h(size.height)
                                    .bg(gpui::black().opacity(0.4))
                                    .occlude(),
                            ),
                    )
                    .with_priority(94),
                )
                .child(
                    popover(
                        "group-dialog",
                        Anchor::Point(gpui::point(
                            px((size.width.as_f32() - width) / 2.),
                            px((size.height.as_f32() - height) / 2.),
                        )),
                    )
                    .ignore(self.notice_region.clone())
                    .modal_surface(true)
                    .without_animation()
                    .priority(95)
                    .width(width)
                    .height(height)
                    .gap(0.)
                    .on_dismiss(move |_, cx| {
                        let _ = weak.update(cx, |this, cx| this.close_group_dialog(cx));
                    })
                    .child(view.clone()),
                )
                .into_any_element(),
        )
    }

    /// The group's Members tab (`GroupMembers`): who is in it, with adding
    /// and removing. The Principal leads every group and cannot leave.
    pub(super) fn members_list(&mut self, mono: &Mono, cx: &mut Context<Self>) -> AnyElement {
        let group = mono.group.clone().unwrap_or_default();
        let brief = native::members(&self.roster.monos);
        let role_of = |id: &str| {
            brief
                .iter()
                .find(|member| member.id == id)
                .map(member_role)
                .unwrap_or_default()
        };
        let gid = mono.id.clone();
        let row = |mono: &Mono, action: Option<AnyElement>| {
            let role = role_of(&mono.id);
            div()
                .id(SharedString::from(format!("group-member-{}", mono.id)))
                .min_h(px(36.))
                .px_2()
                .py_1()
                .flex()
                .items_center()
                .gap_2()
                .child(avatar::avatar(&mono.mascot, mono.color, "idle", 16., true))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(13.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .child(mono.label()),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_size(px(11.))
                                .text_color(content(0.5))
                                .child(role),
                        ),
                )
                .children(action)
                .into_any_element()
        };
        let leads_tag = || {
            div()
                .flex_shrink_0()
                .px_1p5()
                .py_0p5()
                .rounded_md()
                .bg(accent_color().opacity(0.1))
                .text_color(accent_color())
                .text_size(px(11.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child("Leads")
                .into_any_element()
        };
        let current: Vec<AnyElement> = group
            .members
            .iter()
            .filter_map(|id| self.roster.get(id).cloned())
            .map(|member| {
                if member.is_principal() {
                    row(&member, Some(leads_tag()))
                } else {
                    let (gid, member_id) = (gid.clone(), member.id.clone());
                    row(
                        &member,
                        Some(
                            div()
                                .id(SharedString::from(format!(
                                    "group-member-remove-{}",
                                    member.id
                                )))
                                .cursor_pointer()
                                .size(px(24.))
                                .rounded_md()
                                .flex()
                                .items_center()
                                .justify_center()
                                .hover(|s| s.bg(content(0.08)))
                                .child(icon("trash", 14.).text_color(content(0.45)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_group_member(&gid, &member_id, false, cx)
                                }))
                                .into_any_element(),
                        ),
                    )
                }
            })
            .collect();
        let others: Vec<AnyElement> = if self.members_adding {
            self.roster
                .monos
                .iter()
                .filter(|m| !m.is_group() && !group.members.contains(&m.id))
                .map(|member| {
                    let (gid, member_id) = (gid.clone(), member.id.clone());
                    row(
                        member,
                        Some(
                            div()
                                .id(SharedString::from(format!(
                                    "group-member-add-{}",
                                    member.id
                                )))
                                .cursor_pointer()
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .text_size(px(12.))
                                .text_color(accent_color())
                                .hover(|s| s.bg(content(0.06)))
                                .child("Add")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.move_group_member(&gid, &member_id, true, cx)
                                }))
                                .into_any_element(),
                        ),
                    )
                })
                .collect()
        } else {
            Vec::new()
        };
        let full = group.members.len() >= GROUP_MAX_MEMBERS;
        let has_others = self
            .roster
            .monos
            .iter()
            .any(|m| !m.is_group() && !group.members.contains(&m.id));
        div()
            .id("group-members")
            .debug_selector(|| "group-members".into())
            .size_full()
            .overflow_y_scroll()
            .px_2()
            .py_2()
            .flex()
            .flex_col()
            .gap(px(1.))
            .when(current.is_empty(), |d| {
                d.child(panels::empty_state(
                    "users",
                    "Add the agents this group brings together. Its chat leads them and brings each one in when its part comes up.",
                ))
            })
            .children(current)
            .when(self.members_adding && !others.is_empty(), |d| {
                d.child(
                    div()
                        .mt_2()
                        .pt_2()
                        .border_t_1()
                        .border_color(content(0.07))
                        .flex()
                        .flex_col()
                        .gap(px(1.))
                        .children(others),
                )
            })
            .when(has_others && !full && !self.members_adding, |d| {
                d.child(
                    div()
                        .id("group-member-adding")
                        .cursor_pointer()
                        .mt_1()
                        .px_2()
                        .py_1p5()
                        .rounded_md()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_size(px(13.))
                        .text_color(content(0.6))
                        .hover(|s| s.bg(content(0.06)).text_color(content(1.)))
                        .child(icon("plus", 14.))
                        .child("Add member")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.members_adding = true;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    /// Adds or removes a member (`saveGroupMembers`): the Principal's own
    /// seat cannot leave.
    fn move_group_member(&mut self, id: &str, member: &str, add: bool, cx: &mut Context<Self>) {
        let Some(mono) = self.roster.get(id) else {
            return;
        };
        let mut group = mono.group.clone().unwrap_or_default();
        if add {
            if group.members.len() >= GROUP_MAX_MEMBERS
                || group.members.iter().any(|id| id == member)
            {
                return;
            }
            group.members.push(member.into());
        } else {
            group.members.retain(|id| id != member);
        }
        let name = mono.name.clone();
        self.roster.save_group(id, &name, group);
        self.configure_chat(id, cx);
        self.save(cx);
    }
}
