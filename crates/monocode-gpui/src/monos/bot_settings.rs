//! A bot's part of its settings (MonoSettingsDialog.tsx): the Orchestrator
//! badge, its specialty, the Principal's daily brief, and its tools — Claude's
//! own, the user's accounts, MonoBOT's skills, the skills installed for Claude
//! Code, the folders the user opened to it and the app's connectors. Sending,
//! sharing and deleting stay blocked whatever is on.
use super::panels::{card, heading, setting, switch_setting};
use super::*;
use gpui::{ElementId, FontWeight};
use monocode_core::connectors;
use monocode_core::mono::{
    agents,
    bot_tools::{self, BotFolder, BotTools, Tools},
    bots, brief,
};
use monocode_core::time::now_ms;

/// A specialty is one line the orchestrator reads, not a soul.
const SPECIALTY_MAX: usize = 200;
/// Installed skills shown at once; the filter finds the rest.
const SKILLS_SHOWN: usize = 40;

#[derive(Default)]
pub(super) struct BotSettingsState {
    /// The skills installed for Claude Code, sorted; none until loaded.
    installed: Option<Vec<String>>,
    loading: bool,
    /// Why the last folder could not be added.
    folder_problem: Option<String>,
}

/// "Orchestrator" under the Principal's name.
pub(super) fn orchestrator_badge() -> AnyElement {
    div()
        .id("mono-orchestrator-badge")
        .debug_selector(|| "mono-orchestrator-badge".into())
        .mt_1()
        .px(px(6.))
        .rounded_sm()
        .bg(accent_color().opacity(0.1))
        .text_color(accent_color())
        .text_size(px(11.))
        .line_height(px(16.))
        .font_weight(FontWeight::MEDIUM)
        .aria_label(
            "Orchestrator: it does its own job and hands the rest to the agent whose specialty fits.",
        )
        .child("Orchestrator")
        .into_any_element()
}

/// The settings switch, for one item of a bot's tools.
fn toggle(
    id: impl Into<ElementId>,
    label: &str,
    on: bool,
    disabled: bool,
    cx: &mut Context<Monos>,
    change: impl Fn(&mut Monos, &mut Context<Monos>) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label.to_owned())
        .aria_selected(on)
        .w(px(36.))
        .h(px(20.))
        .flex_shrink_0()
        .bg(if on { accent_color() } else { content(0.2) })
        .child(
            div()
                .size(px(16.))
                .mt(px(2.))
                .ml(px(if on { 18. } else { 2. }))
                .bg(gpui::white()),
        )
        .when(!disabled, |d| {
            d.cursor_pointer()
                .on_click(cx.listener(move |this, _, _, cx| change(this, cx)))
        })
        .into_any_element()
}

/// Adds or drops one item of a list.
fn flip(list: &mut Vec<String>, item: &str, on: bool) {
    list.retain(|entry| entry != item);
    if on {
        list.push(item.to_owned());
    }
}

/// The path as the rail writes it: the home folder as `~`.
fn pretty(path: &str) -> String {
    monocode_core::accounts::home()
        .and_then(|home| {
            let home = home.to_string_lossy().into_owned();
            path.strip_prefix(&home).map(|rest| format!("~{rest}"))
        })
        .unwrap_or_else(|| path.to_owned())
}

impl Monos {
    fn bot_tools_of(&self, id: &str) -> Option<Tools> {
        let mono = self.roster.get(id).filter(|m| m.is_bot())?;
        Some(bot_tools::tools(
            mono.bot_role.as_deref().unwrap_or("assistant"),
            mono.bot_tools.as_ref(),
        ))
    }

    /// saveBotTools: the whole list becomes the bot's own, from its next turn.
    pub(super) fn save_bot_tools(
        &mut self,
        id: &str,
        change: impl FnOnce(&mut Tools),
        cx: &mut Context<Self>,
    ) {
        let Some(mut tools) = self.bot_tools_of(id) else {
            return;
        };
        change(&mut tools);
        if let Some(mono) = self.roster.get_mut(id) {
            mono.bot_tools = Some(BotTools::from(tools));
        }
        self.configure_chat(id, cx);
        self.save(cx);
        cx.notify();
    }

    /// The specialty and daily brief time fields, as they are typed.
    pub(super) fn bot_field_changed(&mut self, key: &str, text: &str, cx: &mut Context<Self>) {
        let Some(id) = self.active.clone() else {
            return;
        };
        if key == "mono-specialty" {
            let line: String = text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .chars()
                .take(SPECIALTY_MAX)
                .collect();
            if let Some(mono) = self.roster.get_mut(&id) {
                mono.specialty = (!line.is_empty()).then_some(line);
            }
            self.configure_chat(&id, cx);
            self.save(cx);
        } else if key == "daily-brief-time" {
            let time = text.trim().to_owned();
            let Some(habit) = brief::find(&self.habits).map(|h| h.id.clone()) else {
                return;
            };
            if !brief::is_time(&time) {
                return;
            }
            let root = self.root.clone();
            let mono = id.clone();
            self.file_work(
                id,
                move || {
                    habits::update(&root, &mono, |all| {
                        if let Some(h) = all.iter_mut().find(|h| h.id == habit) {
                            h.schedule.time = time.clone();
                            h.next_run_at = h.schedule.next(now_ms())?;
                        }
                        Ok(())
                    })
                },
                cx,
            );
        }
        cx.notify();
    }

    /// Specialty and the Principal's daily brief, above the model.
    pub(super) fn bot_identity_settings(
        &mut self,
        mono: &Mono,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut parts = Vec::new();
        if mono.is_group() || mono.kind.as_deref() == Some("workstream") {
            return parts;
        }
        let principal = mono.is_principal();
        parts.push(
            heading(
                "Specialty",
                Some(if principal {
                    "What it does itself. Everything else it hands to the agent whose specialty fits."
                } else {
                    "What it takes on, in one line. The orchestrator reads it to pick who gets each job."
                }),
            )
            .into_any_element(),
        );
        let placeholder = if mono.is_bot() {
            bots::role(mono.bot_role.as_deref().unwrap_or("assistant"))
                .map_or("", |role| role.specialty)
                .to_owned()
        } else {
            "What it is best at: frontend, releases, reviews…".to_owned()
        };
        let own = mono.specialty.clone().unwrap_or_default();
        let field = self.field("mono-specialty", "Specialty", own, false, cx);
        field.update(cx, |f, cx| f.set_placeholder(placeholder, cx));
        parts.push(
            div()
                .id("mono-specialty-field")
                .debug_selector(|| "mono-specialty-field".into())
                .w_full()
                .min_h(px(56.))
                .rounded_md()
                .border_1()
                .border_color(content(0.1))
                .bg(background_base())
                .px(px(10.))
                .py(px(8.))
                .text_size(px(13.))
                .line_height(px(20.))
                .child(field)
                .into_any_element(),
        );
        if principal {
            parts.extend(self.daily_brief_settings(mono, cx));
        }
        parts
    }

    /// DailyBriefSetting: the orchestrator's weekday morning brief, a habit.
    fn daily_brief_settings(&mut self, mono: &Mono, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let found = brief::find(&self.habits).cloned();
        let time = found
            .as_ref()
            .map_or(brief::TIME.to_owned(), |h| h.schedule.time.clone());
        let field = self.field("daily-brief-time", "Daily brief time", time, false, cx);
        let id = mono.id.clone();
        let on = found.is_some();
        let switch = toggle("daily-brief", "Daily brief", on, false, cx, {
            let id = id.clone();
            let habit = found.as_ref().map(|h| h.id.clone());
            move |this, cx| {
                let root = this.root.clone();
                let mono = id.clone();
                let habit = habit.clone();
                let time = this.value("daily-brief-time", cx);
                let time = if brief::is_time(time.trim()) {
                    time.trim().to_owned()
                } else {
                    brief::TIME.to_owned()
                };
                this.file_work(
                    id.clone(),
                    move || {
                        habits::update(&root, &mono, |all| {
                            match &habit {
                                Some(habit) => all.retain(|h| &h.id != habit),
                                None => all.push(brief::draft(&time).habit(now_ms())?),
                            }
                            Ok(())
                        })
                    },
                    cx,
                );
            }
        });
        let mut rows = card()
            .child(setting(
                "Daily brief",
                Some("Weekdays. Replies it can write wait in Drafts; it never sends."),
                switch,
            ))
            .child(
                setting(
                    "Time",
                    Some("Local time on this computer."),
                    div()
                        .id("daily-brief-time")
                        .w(px(72.))
                        .h(px(32.))
                        .px_2()
                        .flex()
                        .items_center()
                        .rounded_md()
                        .border_1()
                        .border_color(content(0.1))
                        .bg(background_base())
                        .text_size(px(12.))
                        .child(field)
                        .into_any_element(),
                )
                .border_t_1()
                .border_color(content(0.04)),
            );
        if let Some(habit) = found {
            let last = habit.last_run_at.map_or_else(
                || "It hasn't run yet.".to_owned(),
                |at| {
                    let at = chrono::DateTime::from_timestamp_millis(at as i64)
                        .map(|at| at.with_timezone(&chrono::Local))
                        .map(|at| at.format("%b %-d, %Y, %H:%M").to_string())
                        .unwrap_or_default();
                    format!("Last brief {at}")
                },
            );
            let habit_id = habit.id.clone();
            rows = rows.child(
                setting(
                    "Try it now",
                    Some(&last),
                    div()
                        .id("daily-brief-run")
                        .role(Role::Button)
                        .aria_label("Run the daily brief now")
                        .rounded_md()
                        .px_3()
                        .py_1p5()
                        .text_size(px(12.))
                        .hover(|s| s.bg(content(0.08)))
                        .child("Run now")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let root = this.root.clone();
                            let mono = id.clone();
                            let habit = habit_id.clone();
                            this.file_work(
                                id.clone(),
                                move || {
                                    habits::update(&root, &mono, |all| {
                                        if let Some(h) = all.iter_mut().find(|h| h.id == habit) {
                                            h.run_requested = true;
                                        }
                                        Ok(())
                                    })
                                },
                                cx,
                            );
                        }))
                        .into_any_element(),
                )
                .border_t_1()
                .border_color(content(0.04)),
            );
        }
        vec![
            heading(
                "Daily brief",
                Some("Each weekday morning it goes through your email, what the other agents did and what is due, and tells you what needs you today."),
            )
            .into_any_element(),
            rows.into_any_element(),
        ]
    }

    /// BotToolsGroup, below the model: its tools, installed skills, folders
    /// and connectors.
    pub(super) fn bot_tools_settings(
        &mut self,
        mono: &Mono,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let Some(tools) = self.bot_tools_of(&mono.id) else {
            return Vec::new();
        };
        let id = mono.id.clone();
        let line = || div().border_t_1().border_color(content(0.04));
        let mut list = card().child(setting(
            "Its role's tools",
            Some("Go back to what its role starts with; folders stay."),
            div()
                .id("bot-tools-reset")
                .role(Role::Button)
                .aria_label("Use role's tools")
                .rounded_md()
                .px_3()
                .py_1p5()
                .text_size(px(12.))
                .hover(|s| s.bg(content(0.08)))
                .child("Use role's")
                .on_click(cx.listener({
                    let id = id.clone();
                    move |this, _, _, cx| {
                        if let Some(mono) = this.roster.get_mut(&id) {
                            let role = mono.bot_role.clone().unwrap_or_else(|| "assistant".into());
                            mono.bot_tools = bot_tools::role_reset(&role, mono.bot_tools.as_ref());
                        }
                        this.configure_chat(&id, cx);
                        this.save(cx);
                        cx.notify();
                    }
                }))
                .into_any_element(),
        ));
        for access in bot_tools::ACCESS {
            let (name, note) = agents::access_label(access);
            let on = tools.access.iter().any(|a| a == access);
            let id = id.clone();
            let switch = toggle(
                SharedString::from(format!("bot-access-{access}")),
                name,
                on,
                false,
                cx,
                move |this, cx| this.save_bot_tools(&id, |t| flip(&mut t.access, access, !on), cx),
            );
            list = list.child(line().child(switch_setting(name, note, false, switch)));
        }
        for service in bot_tools::SERVICES {
            let (name, note) = agents::service_label(service);
            let on = tools.services.iter().any(|s| s == service);
            let id = id.clone();
            let switch = toggle(
                SharedString::from(format!("bot-service-{service}")),
                name,
                on,
                false,
                cx,
                move |this, cx| {
                    this.save_bot_tools(&id, |t| flip(&mut t.services, service, !on), cx)
                },
            );
            list = list.child(line().child(switch_setting(name, note, false, switch)));
        }
        for plugin in bot_tools::PLUGINS {
            let (name, note) = agents::pack_label(plugin);
            let label = format!("{name} skill");
            let on = tools.plugins.iter().any(|p| p == plugin);
            let id = id.clone();
            let switch = toggle(
                SharedString::from(format!("bot-plugin-{plugin}")),
                &label,
                on,
                false,
                cx,
                move |this, cx| this.save_bot_tools(&id, |t| flip(&mut t.plugins, plugin, !on), cx),
            );
            list = list.child(line().child(switch_setting(&label, note, false, switch)));
        }
        vec![
            heading(
                "Tools",
                Some("What it can use, picked for its specialty. Sending, sharing and deleting stay blocked. It can also change these when you ask it in its chat, except folders and the iPhone."),
            )
            .into_any_element(),
            list.into_any_element(),
            heading(
                "Installed skills",
                Some("Skills you installed for Claude Code. The ones it is not given stay closed to it."),
            )
            .into_any_element(),
            self.installed_skills(&id, &tools, cx),
            heading(
                "Folders",
                Some("Beyond its home folder. It reads them without asking; it changes files only where you tick it, and never deletes."),
            )
            .into_any_element(),
            self.bot_folders(&id, &tools, cx),
            heading(
                "Connectors",
                Some("The app's connectors it may use, among the ones on in Settings › Connectors."),
            )
            .into_any_element(),
            self.bot_connectors(&id, &tools, cx),
        ]
    }

    /// InstalledSkills: all of them, or the ones its list names, found with
    /// a filter when there are many.
    fn installed_skills(&mut self, id: &str, tools: &Tools, cx: &mut Context<Self>) -> AnyElement {
        let Some(installed) = self.bot_settings.installed.clone() else {
            if !self.bot_settings.loading {
                self.bot_settings.loading = true;
                let home = monocode_core::accounts::home().unwrap_or_default();
                let work = cx.background_executor().spawn(async move {
                    let mut names = bot_tools::installed_skills(&home);
                    names.sort();
                    names
                });
                cx.spawn(async move |this, cx| {
                    let names = work.await;
                    let _ = this.update(cx, |this, cx| {
                        this.bot_settings.installed = Some(names);
                        this.bot_settings.loading = false;
                        cx.notify();
                    });
                })
                .detach();
            }
            return card().into_any_element();
        };
        if installed.is_empty() {
            return card()
                .child(
                    div()
                        .px_3()
                        .py_2()
                        .text_size(px(12.))
                        .text_color(content(0.4))
                        .child("No skills installed for Claude Code."),
                )
                .into_any_element();
        }
        let allowed = tools.skills.clone();
        let on = |name: &str| {
            allowed.as_ref().is_none_or(|list| {
                list.iter()
                    .any(|wanted| bot_tools::skill_matches(wanted, name))
            })
        };
        let count = installed.iter().filter(|name| on(name)).count();
        let every = toggle(
            "bot-skills-all",
            "Every installed skill",
            allowed.is_none(),
            false,
            cx,
            {
                let id = id.to_owned();
                let all = allowed.is_none();
                move |this, cx| {
                    this.save_bot_tools(
                        &id,
                        |t| t.skills = if all { Some(Vec::new()) } else { None },
                        cx,
                    )
                }
            },
        );
        let note = match &allowed {
            Some(_) => format!("{count} of {} picked for its job", installed.len()),
            None => format!("All {} are open to it", installed.len()),
        };
        let mut group = card().child(switch_setting("Every installed skill", &note, false, every));
        if let Some(list) = allowed.clone() {
            let filter = self.field(
                "bot-skills-filter",
                &format!("Filter {} skills", installed.len()),
                String::new(),
                false,
                cx,
            );
            let needle = self.value("bot-skills-filter", cx).trim().to_lowercase();
            let mut shown: Vec<&String> = installed
                .iter()
                .filter(|name| needle.is_empty() || name.to_lowercase().contains(&needle))
                .collect();
            // The ones it has first, so they are never lost below the cut.
            shown.sort_by_key(|name| !on(name));
            group = group.child(
                div()
                    .px_3()
                    .py_2()
                    .border_t_1()
                    .border_color(content(0.04))
                    .child(
                        div()
                            .id("bot-skills-filter")
                            .h(px(36.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(content(0.1))
                            .bg(background_base())
                            .text_size(px(13.))
                            .child(filter),
                    ),
            );
            for (ix, name) in shown.into_iter().take(SKILLS_SHOWN).enumerate() {
                let has = on(name);
                let id = id.to_owned();
                let skill = name.clone();
                let list = list.clone();
                let switch = toggle(("bot-skill", ix), name, has, false, cx, move |this, cx| {
                    let mut next = list.clone();
                    if has {
                        next.retain(|wanted| !bot_tools::skill_matches(wanted, &skill));
                    } else {
                        next.push(skill.clone());
                    }
                    this.save_bot_tools(&id, |t| t.skills = Some(next), cx);
                });
                let note = if has {
                    "It may use this skill"
                } else {
                    "Closed to it"
                };
                group = group.child(
                    div()
                        .border_t_1()
                        .border_color(content(0.04))
                        .child(switch_setting(name, note, false, switch)),
                );
            }
        }
        group.into_any_element()
    }

    /// BotFolders: the folders the user opened to it beyond its home. Only
    /// the user adds them, so nothing a bot reads can widen what it touches.
    fn bot_folders(&mut self, id: &str, tools: &Tools, cx: &mut Context<Self>) -> AnyElement {
        let can_edit = tools.access.iter().any(|a| a == "edit");
        let mut group = card();
        for (ix, folder) in tools.folders.iter().enumerate() {
            let edit_problem = bot_tools::folder_problem(&folder.path, true);
            let path = folder.path.clone();
            let row_group: SharedString = format!("bot-folder-{ix}").into();
            let control: AnyElement = if can_edit && edit_problem.is_none() {
                let id = id.to_owned();
                let path = path.clone();
                let on = folder.edit;
                div()
                    .id(("bot-folder-edit", ix))
                    .role(Role::CheckBox)
                    .aria_label("Can change")
                    .aria_selected(on)
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.55))
                    .cursor_pointer()
                    .child(
                        div()
                            .size(px(14.))
                            .border_1()
                            .border_color(content(if on { 0. } else { 0.3 }))
                            .when(on, |d| d.bg(accent_color()))
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(on, |d| {
                                d.child(icon("check", 10.).text_color(gpui::white()))
                            }),
                    )
                    .child("Can change")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let path = path.clone();
                        this.save_bot_tools(
                            &id,
                            |t| {
                                for folder in &mut t.folders {
                                    if folder.path == path {
                                        folder.edit = !on;
                                    }
                                }
                            },
                            cx,
                        );
                    }))
                    .into_any_element()
            } else {
                div()
                    .id(("bot-folder-readonly", ix))
                    .text_size(px(12.))
                    .text_color(content(0.4))
                    .when_some(edit_problem, |d, why| {
                        d.aria_label(format!("Read only: {why}"))
                    })
                    .child("Read only")
                    .into_any_element()
            };
            let remove = {
                let id = id.to_owned();
                let path = path.clone();
                div()
                    .id(("bot-folder-remove", ix))
                    .role(Role::Button)
                    .aria_label(format!("Remove {}", pretty(&path)))
                    .size(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .text_color(content(0.45))
                    .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    .child(icon("close", 14.))
                    .opacity(0.)
                    .group_hover(row_group.clone(), |s| s.opacity(1.))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let path = path.clone();
                        this.save_bot_tools(&id, |t| t.folders.retain(|f| f.path != path), cx);
                    }))
            };
            group = group.child(
                div()
                    .id(("bot-folder", ix))
                    .group(row_group)
                    .min_h(px(48.))
                    .flex()
                    .items_center()
                    .gap_3()
                    .px_3()
                    .py_2()
                    .when(ix > 0, |d| d.border_t_1().border_color(content(0.04)))
                    .child(
                        div()
                            .size(px(32.))
                            .flex_shrink_0()
                            .rounded_lg()
                            .bg(content(0.05))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon("folder", 16.).text_color(content(0.6))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(content(0.9))
                            .child(pretty(&path)),
                    )
                    .child(control)
                    .child(remove),
            );
        }
        let add = id.to_owned();
        group = group.child(
            div()
                .id("bot-folder-add")
                .role(Role::Button)
                .aria_label("Add folder")
                .h(px(44.))
                .px_3()
                .flex()
                .items_center()
                .gap_3()
                .text_size(px(13.))
                .text_color(content(0.55))
                .hover(|s| s.bg(content(0.04)).text_color(content(1.)))
                .when(!tools.folders.is_empty(), |d| {
                    d.border_t_1().border_color(content(0.04))
                })
                .child(
                    div()
                        .size(px(32.))
                        .rounded_lg()
                        .border_1()
                        .border_color(content(0.15))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon("plus", 14.)),
                )
                .child("Add folder")
                .on_click(cx.listener(move |this, _, _, cx| this.pick_bot_folders(&add, cx))),
        );
        if let Some(problem) = self.bot_settings.folder_problem.clone() {
            group = group.child(
                div()
                    .id("bot-folder-problem")
                    .role(Role::Alert)
                    .px_3()
                    .py_2()
                    .text_size(px(12.))
                    .text_color(rgb(0xff6467))
                    .child(problem),
            );
        }
        group.into_any_element()
    }

    /// The system picker, then each folder that may be opened to a bot.
    fn pick_bot_folders(&mut self, id: &str, cx: &mut Context<Self>) {
        self.bot_settings.folder_problem = None;
        let picked = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Open folders to this bot".into()),
        });
        let id = id.to_owned();
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |this, cx| {
                let Some(tools) = this.bot_tools_of(&id) else {
                    return;
                };
                let mut next = tools.folders.clone();
                let mut problem = None;
                for path in paths {
                    let path = path.to_string_lossy().into_owned();
                    if let Some(why) = bot_tools::folder_problem(&path, false) {
                        problem = Some(format!("{}: {why}", pretty(&path)));
                        continue;
                    }
                    if !next.iter().any(|f| f.path == path) {
                        next.push(BotFolder { path, edit: false });
                    }
                }
                if next.len() > bot_tools::FOLDERS_MAX {
                    problem = Some(format!("At most {} folders", bot_tools::FOLDERS_MAX));
                    next.truncate(bot_tools::FOLDERS_MAX);
                }
                this.bot_settings.folder_problem = problem;
                this.save_bot_tools(&id, |t| t.folders = next, cx);
            });
        })
        .detach();
    }

    /// The app's connectors it may use, among the ones on in Settings ›
    /// Connectors.
    fn bot_connectors(&mut self, id: &str, tools: &Tools, cx: &mut Context<Self>) -> AnyElement {
        let rows = connectors::settings_rows();
        // Without a list it had every connector but the ones the user grants
        // by name; the list keeps exactly that.
        let current: Vec<String> = tools.connectors.clone().unwrap_or_else(|| {
            rows.iter()
                .filter(|row| !row.user_granted)
                .map(|row| row.id.clone())
                .collect()
        });
        let mut group = card();
        for (ix, row) in rows.into_iter().enumerate() {
            let listed = match &tools.connectors {
                Some(list) => list.iter().any(|c| c == &row.id),
                None => !row.user_granted,
            };
            let on = row.ready && listed;
            let note = if row.ready {
                row.description.clone()
            } else if row.enabled {
                "Not set up yet: Settings › Connectors".to_owned()
            } else {
                "Off in Settings › Connectors".to_owned()
            };
            let id = id.to_owned();
            let current = current.clone();
            let connector = row.id.clone();
            let switch = toggle(
                SharedString::from(format!("bot-connector-{}", row.id)),
                &row.name,
                on,
                !row.ready,
                cx,
                move |this, cx| {
                    let mut next = current.clone();
                    flip(&mut next, &connector, !on);
                    this.save_bot_tools(&id, |t| t.connectors = Some(next), cx);
                },
            );
            group = group.child(
                div()
                    .when(ix > 0, |d| d.border_t_1().border_color(content(0.04)))
                    .child(switch_setting(&row.name, &note, !row.ready, switch)),
            );
        }
        group.into_any_element()
    }
}
