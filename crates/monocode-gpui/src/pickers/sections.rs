//! The composer's model picker, after Capy 0.9's: favorites first, each at
//! the reasoning effort it opens at and numbered for ⌥1–⌥9, then every
//! provider's models in one list. Beside the list, a panel shows the
//! highlighted model's options: reasoning, fast mode, thinking, service
//! tier, the provider's own controls, the account it runs on, and whether
//! it is a favorite. Changing one switches to that model and applies the
//! option through the composer's own settings actions (`effort:`, `fast:on`,
//! `usage-account:` …), so the picker holds no settings logic of its own.
use super::*;
use crate::settings::ModelChoice;
use std::collections::HashMap;

/// The provider rail, the list beside it and the options panel.
const RAIL_WIDTH: f32 = 40.;
const LIST_WIDTH: f32 = 300.;
const PANEL_WIDTH: f32 = 264.;
pub(crate) const PICKER_WIDTH: f32 = RAIL_WIDTH + LIST_WIDTH + PANEL_WIDTH;
pub(crate) const PICKER_HEIGHT: f32 = 432.;

/// The session's settings for its current model, as the panel shows them.
#[derive(Clone, Default, PartialEq, Debug)]
pub(crate) struct PanelState {
    pub effort: Option<String>,
    pub fast: bool,
    pub thinking: Option<bool>,
    pub tier: Option<String>,
    pub model_settings: HashMap<String, String>,
    /// The account the session runs on.
    pub account: String,
}

/// A change made in the panel: switch to `model` when it is not the
/// session's, then apply `action` as the composer's settings menu does.
pub(crate) struct ModelOption {
    pub kind: Kind,
    pub model: Option<String>,
    pub action: SharedString,
}

/// ⌥1–⌥9 in a session: switch to that favorite at its effort.
#[derive(Clone, Debug, PartialEq, gpui::Action)]
#[action(namespace = model_picker, no_json)]
pub(crate) struct PickFavorite(pub usize);

pub(crate) fn bind(cx: &mut gpui::App) {
    cx.bind_keys(
        (1..=9).map(|n| {
            gpui::KeyBinding::new(&format!("alt-{n}"), PickFavorite(n - 1), Some("Session"))
        }),
    );
}

/// The favorite at `index`, with the effort it opens at.
pub(crate) fn favorite_choice(
    index: usize,
    cx: &gpui::App,
) -> Option<(Kind, Option<String>, Option<String>)> {
    let prefs = cx.try_global::<crate::settings::Preferences>()?;
    let choice = prefs.favorite_models.get(index)?;
    let kind = Kind::parse(&choice.provider)?;
    let effort = prefs
        .favorite_efforts
        .get(&ModelChoice::key(&choice.provider, choice.model.as_deref()))
        .cloned();
    Some((kind, choice.model.clone(), effort))
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Section {
    Favorites,
    Provider(Kind),
}

pub(super) struct SectionRow {
    pub entry: ModelEntry,
    pub section: Section,
    /// A favorite's saved effort.
    pub effort: Option<String>,
    /// A favorite's ⌥ number, 1–9.
    pub number: Option<usize>,
}

impl ModelList {
    /// Show every provider in one list beside the options panel, holding the
    /// session's settings for its current model.
    pub(crate) fn sections(mut self, state: PanelState, cx: &mut Context<Self>) -> Self {
        self.sections = Some(state);
        self.search
            .update(cx, |search, cx| search.set_placeholder("Search models", cx));
        for kind in self.visible_kinds() {
            if !self.catalogs.iter().any(|(k, _)| *k == kind) {
                self.discover(kind, cx);
            }
        }
        self.highlighted = self.current_index(cx);
        self
    }

    /// Follow the session after an option was applied.
    pub(crate) fn set_panel_state(
        &mut self,
        kind: Kind,
        model: Option<String>,
        state: PanelState,
        cx: &mut Context<Self>,
    ) {
        self.provider = kind;
        self.current = model;
        self.sections = Some(state);
        cx.notify();
    }

    pub(super) fn section_rows(&self, cx: &gpui::App) -> Vec<SectionRow> {
        let query = self.query(cx);
        let mut rows = Vec::new();
        if let Some(prefs) = cx.try_global::<crate::settings::Preferences>() {
            for (ix, choice) in prefs.favorite_models.iter().enumerate() {
                let Some(kind) = Kind::parse(&choice.provider) else {
                    continue;
                };
                if kind != self.provider && self.hidden.iter().any(|id| id == kind.id()) {
                    continue;
                }
                let name = self.saved_model_name(kind, choice.model.as_deref());
                if !query.is_empty()
                    && !format!("{name} {}", kind.label())
                        .to_lowercase()
                        .contains(&query)
                {
                    continue;
                }
                rows.push(SectionRow {
                    entry: ModelEntry {
                        kind,
                        id: choice.model.clone(),
                        name,
                        enabled: self.enabled(kind),
                    },
                    section: Section::Favorites,
                    effort: prefs
                        .favorite_efforts
                        .get(&ModelChoice::key(&choice.provider, choice.model.as_deref()))
                        .cloned(),
                    number: (ix < 9).then_some(ix + 1),
                });
            }
        }
        let mut kinds = self.visible_kinds();
        // The session's own provider leads.
        kinds.sort_by_key(|kind| *kind != self.provider);
        for kind in kinds {
            rows.extend(
                self.kind_entries(kind, cx)
                    .into_iter()
                    .map(|entry| SectionRow {
                        entry,
                        section: Section::Provider(kind),
                        effort: None,
                        number: None,
                    }),
            );
        }
        rows
    }

    fn is_current(&self, kind: Kind, id: Option<&str>) -> bool {
        let stripped = self
            .current
            .as_deref()
            .and_then(|c| c.strip_prefix(&format!("{}:", kind.id())));
        kind == self.provider
            && (self.current.as_deref() == id || (stripped.is_some() && stripped == id))
    }

    /// The saved favorite matching this model, as stored.
    fn favorite_key(&self, kind: Kind, id: Option<&str>, cx: &gpui::App) -> Option<String> {
        let stripped = id.and_then(|i| i.strip_prefix(&format!("{}:", kind.id())).or(Some(i)));
        cx.try_global::<crate::settings::Preferences>()?
            .favorite_models
            .iter()
            .find(|c| {
                c.provider == kind.id()
                    && (c.model.as_deref() == id || c.model.as_deref() == stripped)
            })
            .map(|c| ModelChoice::key(&c.provider, c.model.as_deref()))
    }

    /// A favorite's ⌥ number, when it has one.
    fn favorite_number(&self, kind: Kind, id: Option<&str>, cx: &gpui::App) -> Option<usize> {
        let key = self.favorite_key(kind, id, cx)?;
        cx.try_global::<crate::settings::Preferences>()?
            .favorite_models
            .iter()
            .position(|c| ModelChoice::key(&c.provider, c.model.as_deref()) == key)
            .filter(|ix| *ix < 9)
            .map(|ix| ix + 1)
    }

    /// Picking a row: a favorite brings its effort along.
    pub(super) fn pick_section(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(row) = self.section_rows(cx).into_iter().nth(ix) else {
            return;
        };
        if !row.entry.enabled {
            return;
        }
        if let Some(effort) = row.effort {
            cx.emit(ModelOption {
                kind: row.entry.kind,
                model: row.entry.id.clone(),
                action: format!("effort:{effort}").into(),
            });
        }
        remember_model(row.entry.kind, row.entry.id.clone(), cx);
        cx.emit(ModelPicked(row.entry.kind, row.entry.id));
    }

    fn option(
        &mut self,
        kind: Kind,
        model: Option<String>,
        action: String,
        cx: &mut Context<Self>,
    ) {
        // The effort picked for a favorite is the one it opens at from now on.
        if let Some(effort) = action.strip_prefix("effort:")
            && let Some(key) = self.favorite_key(kind, model.as_deref(), cx)
        {
            let effort = effort.to_owned();
            cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
                prefs.favorite_efforts.insert(key, effort);
            });
        }
        cx.emit(ModelOption {
            kind,
            model,
            action: action.into(),
        });
        cx.notify();
    }

    /// Add or remove a favorite; a new one keeps the effort shown for it.
    fn toggle_section_favorite(
        &mut self,
        kind: Kind,
        id: Option<String>,
        effort: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if !cx.has_global::<crate::settings::Preferences>() {
            return;
        }
        let existing = self.favorite_key(kind, id.as_deref(), cx);
        let section = self
            .section_rows(cx)
            .get(self.highlighted)
            .map(|row| row.section);
        let id_for_prefs = id.clone();
        cx.update_global::<crate::settings::Preferences, _>(|prefs, _| match existing {
            Some(key) => {
                prefs
                    .favorite_models
                    .retain(|c| ModelChoice::key(&c.provider, c.model.as_deref()) != key);
                prefs.favorite_efforts.remove(&key);
            }
            None => {
                let key = ModelChoice::key(kind.id(), id_for_prefs.as_deref());
                prefs.favorite_models.push(ModelChoice {
                    provider: kind.id().into(),
                    model: id_for_prefs,
                });
                if let Some(effort) = effort {
                    prefs.favorite_efforts.insert(key, effort);
                }
            }
        });
        // The rows above may have changed; stay on the same model, in the
        // same section when it is still there.
        let rows = self.section_rows(cx);
        let same = |row: &SectionRow| row.entry.kind == kind && row.entry.id == id;
        if let Some(ix) = rows
            .iter()
            .position(|row| same(row) && Some(row.section) == section)
            .or_else(|| {
                rows.iter()
                    .position(|row| same(row) && row.section != Section::Favorites)
            })
        {
            self.highlighted = ix;
        }
        cx.notify();
    }

    pub(super) fn render_sections(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let rows = self.section_rows(cx);
        let highlighted = rows.get(self.highlighted).map(|row| {
            (
                row.entry.kind,
                row.entry.id.clone(),
                row.entry.name.clone(),
                row.entry.enabled,
                row.effort.clone(),
            )
        });
        let current_section = rows.get(self.highlighted).map(|row| row.section);
        let mut list: Vec<AnyElement> = Vec::new();
        // Each section's header in the list and its first row, for the rail.
        let mut jumps: Vec<(Section, usize, usize)> = Vec::new();
        let mut last: Option<Section> = None;
        for (ix, row) in rows.into_iter().enumerate() {
            if last != Some(row.section) {
                jumps.push((row.section, list.len(), ix));
                list.push(self.section_header(row.section, last.is_some(), cx));
                last = Some(row.section);
            }
            list.push(self.section_row(ix, row, cx));
        }
        let rail = div()
            .id("model-rail")
            .role(Role::TabList)
            .aria_label("Providers")
            .w(px(RAIL_WIDTH))
            .flex_shrink_0()
            .h_full()
            .py_2()
            .border_r_1()
            .border_color(content(0.07))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .children(jumps.into_iter().map(|(section, child, row)| {
                let (id, label): (SharedString, SharedString) = match section {
                    Section::Favorites => ("model-rail-favorites".into(), "Favorites".into()),
                    Section::Provider(kind) => (
                        format!("model-rail-{}", kind.id()).into(),
                        kind.label().into(),
                    ),
                };
                let selected = current_section == Some(section);
                let selector = id.clone();
                div()
                    .id(id)
                    .debug_selector(move || selector.to_string())
                    .role(Role::Tab)
                    .aria_label(label.clone())
                    .aria_selected(selected)
                    .tooltip(chrome_tip(label, None))
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .when(selected, |d| d.bg(content(0.1)))
                    .when(!selected, |d| d.hover(|s| s.bg(content(0.06))))
                    .child(match section {
                        Section::Favorites => icon("star", 16.)
                            .text_color(content(if selected { 1. } else { 0.5 }))
                            .into_any_element(),
                        Section::Provider(kind) => provider_icon(kind.id(), 16.).into_any_element(),
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.list_scroll.scroll_to_top_of_item(child);
                        this.highlighted = row;
                        cx.notify();
                    }))
            }));
        if list.is_empty() {
            list.push(
                div()
                    .px_2()
                    .py_3()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child("No matching models")
                    .into_any_element(),
            );
        }
        div()
            .id("model-picker")
            .role(Role::Dialog)
            .aria_label("Models")
            .w(px(PICKER_WIDTH))
            .h(px(PICKER_HEIGHT))
            .flex()
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.highlighted, cx);
                cx.stop_propagation();
            }))
            .child(rail)
            .child(
                div()
                    .w(px(LIST_WIDTH))
                    .flex_shrink_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_3()
                            .py(px(10.))
                            .text_color(content(1.))
                            .child(icon("search", 14.).text_color(content(0.5)))
                            .child(div().flex_1().min_w_0().child(self.search.clone())),
                    )
                    .child(
                        div()
                            .id("model-list")
                            .role(Role::ListBox)
                            .aria_label("Models")
                            .track_scroll(&self.list_scroll)
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_1()
                            .pb_1()
                            .flex()
                            .flex_col()
                            .children(list),
                    ),
            )
            .child(
                div()
                    .id("model-options")
                    .role(Role::Group)
                    .aria_label("Model options")
                    .w(px(PANEL_WIDTH))
                    .flex_shrink_0()
                    .h_full()
                    .border_l_1()
                    .border_color(content(0.07))
                    .overflow_y_scroll()
                    .when_some(highlighted, |d, (kind, id, name, enabled, effort)| {
                        d.child(self.panel(kind, id, name, enabled, effort, cx))
                    }),
            )
            .into_any_element()
    }

    fn section_header(&self, section: Section, rule: bool, cx: &mut Context<Self>) -> AnyElement {
        let status = match section {
            Section::Provider(kind) if self.loading.contains(&kind) => Some(("Loading…", None)),
            Section::Provider(kind) => self
                .errors
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, error)| ("Retry", Some((kind, error.clone())))),
            Section::Favorites => None,
        };
        div()
            .flex_shrink_0()
            .when(rule, |d| {
                d.mt_1().border_t_1().border_color(content(0.07)).pt_1()
            })
            .child(
                div()
                    .h(px(28.))
                    .px_2()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .map(|d| match section {
                        Section::Favorites => d.child("Favorites"),
                        Section::Provider(kind) => d
                            .child(provider_icon(kind.id(), 12.).opacity(0.7))
                            .child(kind.label()),
                    })
                    .child(div().flex_1())
                    .when_some(status, |d, (label, retry)| match retry {
                        None => d.child(label),
                        Some((kind, error)) => d.child(
                            div()
                                .id(SharedString::from(format!("retry-models-{}", kind.id())))
                                .role(Role::Button)
                                .aria_label("Retry model discovery")
                                .tooltip(text_tip(error))
                                .cursor_pointer()
                                .hover(|s| s.text_color(content(1.)))
                                .child(label)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.discover(kind, cx);
                                    cx.notify();
                                })),
                        ),
                    }),
            )
            .into_any_element()
    }

    fn section_row(&self, ix: usize, row: SectionRow, cx: &mut Context<Self>) -> AnyElement {
        let entry = row.entry;
        let highlighted = self.highlighted == ix && entry.enabled;
        let current = self.is_current(entry.kind, entry.id.as_deref());
        div()
            .id(("model-option", ix))
            .debug_selector(move || format!("model-option-{ix}"))
            .role(Role::ListBoxOption)
            .aria_label(entry.name.clone())
            .aria_selected(current)
            .flex_shrink_0()
            .h(px(32.))
            .px_2()
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(13.))
            .text_color(content(if entry.enabled { 1. } else { 0.3 }))
            .when(entry.enabled, |d| d.cursor_pointer())
            .when(highlighted, |d| d.bg(content(0.1)))
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered && this.highlighted != ix {
                    this.highlighted = ix;
                    cx.notify();
                }
            }))
            .on_click(cx.listener(move |this, _, _, cx| this.pick(ix, cx)))
            .child(provider_icon(entry.kind.id(), 16.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_baseline()
                    .gap(px(6.))
                    .child(div().min_w_0().truncate().child(entry.name.clone()))
                    .when_some(row.effort, |d, effort| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_color(content(0.45))
                                .child(crate::conversation::effort_name(&effort)),
                        )
                    }),
            )
            .when(current, |d| {
                d.child(icon("check-2", 14.).text_color(content(0.6)))
            })
            .when_some(row.number, |d, number| {
                d.child(
                    div()
                        .w(px(12.))
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .text_color(content(0.4))
                        .font_features(crate::right_panel::tabular())
                        .child(number.to_string()),
                )
            })
            .into_any_element()
    }

    /// The highlighted model's options.
    fn panel(
        &self,
        kind: Kind,
        id: Option<String>,
        name: SharedString,
        enabled: bool,
        saved_effort: Option<String>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = self.is_current(kind, id.as_deref());
        let state = if current {
            self.sections.clone().unwrap_or_default()
        } else {
            PanelState {
                effort: saved_effort,
                account: self
                    .profiles
                    .get(&kind)
                    .map(|p| p.id.clone())
                    .unwrap_or_default(),
                ..PanelState::default()
            }
        };
        let model = self.catalog_model(kind, id.as_deref()).cloned();
        let number = self.favorite_number(kind, id.as_deref(), cx);
        let starred = self.favorite_key(kind, id.as_deref(), cx).is_some();
        let mut blocks: Vec<AnyElement> = Vec::new();
        if let Some(model) = &model {
            let effort = state
                .effort
                .clone()
                .or_else(|| model.default_effort.clone())
                .or_else(|| model.efforts.first().cloned());
            if !model.efforts.is_empty() {
                blocks.push(self.reasoning(
                    kind,
                    &id,
                    &model.efforts,
                    effort.as_deref(),
                    enabled,
                    cx,
                ));
            }
            if model.fast {
                let on = state.fast;
                blocks.push(self.switch_row(
                    "model-fast",
                    "Fast mode",
                    on,
                    kind,
                    &id,
                    if on { "fast:off" } else { "fast:on" },
                    enabled,
                    cx,
                ));
            }
            if model.thinking {
                blocks.push(self.switch_row(
                    "model-thinking",
                    "Thinking",
                    state.thinking.unwrap_or(false),
                    kind,
                    &id,
                    "thinking",
                    enabled,
                    cx,
                ));
            }
            if !model.tiers.is_empty() {
                let mut choices = vec![("tier:".to_owned(), "Standard".to_owned())];
                choices.extend(
                    model
                        .tiers
                        .iter()
                        .map(|(id, name)| (format!("tier:{id}"), name.clone())),
                );
                let selected = format!("tier:{}", state.tier.clone().unwrap_or_default());
                blocks.push(self.choice_row(
                    "model-tier",
                    "Service tier",
                    choices,
                    &selected,
                    kind,
                    &id,
                    enabled,
                    cx,
                ));
            }
            for control in &model.controls {
                if kind == Kind::OpenCode && control.id == "agent" {
                    continue;
                }
                let value = state
                    .model_settings
                    .get(&control.id)
                    .cloned()
                    .unwrap_or_else(|| control.current.clone());
                let label = match control.id.as_str() {
                    "effort" | "reasoning" => "Effort".to_owned(),
                    _ => control.name.clone(),
                };
                if crate::conversation::model_control_toggle(control) {
                    blocks.push(self.switch_row(
                        SharedString::from(format!("model-control-{}", control.id)),
                        label,
                        value == "true",
                        kind,
                        &id,
                        &format!("model-toggle:{}", control.id),
                        enabled,
                        cx,
                    ));
                    continue;
                }
                let action = |choice: &str| {
                    format!(
                        "model-setting:{}",
                        serde_json::to_string(&(control.id.clone(), choice.to_owned()))
                            .unwrap_or_default()
                    )
                };
                let choices = control
                    .choices
                    .iter()
                    .map(|(value, name)| (action(value), name.clone()))
                    .collect();
                blocks.push(self.choice_row(
                    SharedString::from(format!("model-control-{}", control.id)),
                    label,
                    choices,
                    &action(&value),
                    kind,
                    &id,
                    enabled,
                    cx,
                ));
            }
        } else if self.loading.contains(&kind) {
            blocks.push(
                div()
                    .text_size(px(12.))
                    .text_color(content(0.5))
                    .child("Loading options…")
                    .into_any_element(),
            );
        }
        // The accounts this provider can run on, when there is a choice.
        let accounts: Vec<(String, String)> = crate::accounts::get(cx)
            .map(|center| {
                center
                    .read(cx)
                    .book
                    .list(kind)
                    .into_iter()
                    .map(|account| (format!("usage-account:{}", account.id), account.label))
                    .collect()
            })
            .unwrap_or_default();
        if accounts.len() > 1 {
            let selected = format!("usage-account:{}", state.account);
            blocks.push(self.choice_row(
                "model-account",
                "Runs on",
                accounts,
                &selected,
                kind,
                &id,
                enabled,
                cx,
            ));
        }
        let (star_kind, star_id) = (kind, id.clone());
        let star_effort = model.as_ref().and_then(|m| {
            state
                .effort
                .clone()
                .or_else(|| m.default_effort.clone())
                .filter(|_| !m.efforts.is_empty())
        });
        div()
            .p_3()
            .min_h_full()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(provider_icon(kind.id(), 20.))
                    .child(
                        div()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(14.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(content(1.))
                                    .child(name),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.5))
                                    .child(kind.label()),
                            ),
                    ),
            )
            .children(blocks)
            .child(div().flex_1())
            .child(
                div()
                    .id("model-favorite")
                    .debug_selector(|| "model-favorite".into())
                    .role(Role::Button)
                    .aria_label(if starred {
                        "Remove from favorites"
                    } else {
                        "Add to favorites"
                    })
                    .h(px(30.))
                    .rounded(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(6.))
                    .text_size(px(13.))
                    .text_color(content(0.9))
                    .bg(content(0.06))
                    .cursor_pointer()
                    .hover(|s| s.bg(content(0.1)))
                    .child(
                        icon(if starred { "star-filled" } else { "star" }, 14.)
                            .text_color(content(if starred { 0.8 } else { 0.5 })),
                    )
                    .child(if starred {
                        "Favorite"
                    } else {
                        "Add to favorites"
                    })
                    .when_some(number.filter(|_| starred), |d, number| {
                        d.child(
                            div()
                                .text_color(content(0.45))
                                .font_features(crate::right_panel::tabular())
                                .child(format!("⌥{number}")),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.toggle_section_favorite(
                            star_kind,
                            star_id.clone(),
                            star_effort.clone(),
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }

    /// Reasoning as a stepped track: one step per effort, filled up to the
    /// one chosen, its name on the right of the label.
    fn reasoning(
        &self,
        kind: Kind,
        id: &Option<String>,
        efforts: &[String],
        selected: Option<&str>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let at = selected.and_then(|s| efforts.iter().position(|e| e == s));
        let label = if kind == Kind::Codex {
            "Reasoning"
        } else {
            "Effort"
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(12.))
                    .child(div().text_color(content(0.5)).child(label))
                    .child(
                        div().text_color(content(1.)).child(
                            selected
                                .map(crate::conversation::effort_name)
                                .unwrap_or_default(),
                        ),
                    ),
            )
            .child(
                div()
                    .id("model-reasoning")
                    .role(Role::RadioGroup)
                    .aria_label(label)
                    .h(px(24.))
                    .p(px(2.))
                    .rounded(px(8.))
                    .bg(content(0.06))
                    .flex()
                    .gap(px(2.))
                    .children(efforts.iter().enumerate().map(|(ix, effort)| {
                        let filled = at.is_some_and(|at| ix <= at);
                        let chosen = at == Some(ix);
                        let action = format!("effort:{effort}");
                        let model = id.clone();
                        div()
                            .id(("model-effort", ix))
                            .debug_selector(move || format!("model-effort-{ix}"))
                            .role(Role::RadioButton)
                            .aria_label(crate::conversation::effort_name(effort))
                            .aria_selected(chosen)
                            .tooltip(chrome_tip(crate::conversation::effort_name(effort), None))
                            .flex_1()
                            .h_full()
                            .rounded(px(6.))
                            .bg(if chosen {
                                accent_color()
                            } else if filled {
                                accent_color().opacity(0.4)
                            } else {
                                gpui::transparent_black()
                            })
                            .when(enabled, |d| {
                                d.cursor_pointer()
                                    .when(!filled, |d| d.hover(|s| s.bg(content(0.08))))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.option(kind, model.clone(), action.clone(), cx)
                                    }))
                            })
                    })),
            )
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn switch_row(
        &self,
        element: impl Into<SharedString>,
        label: impl Into<SharedString>,
        on: bool,
        kind: Kind,
        id: &Option<String>,
        action: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = label.into();
        let (model, action) = (id.clone(), action.to_owned());
        div()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(13.))
            .text_color(content(0.9))
            .child(label.clone())
            .child(
                switch(element, label, on)
                    .when(!enabled, |d| d.opacity(0.4))
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            this.option(kind, model.clone(), action.clone(), cx)
                        }))
                    }),
            )
            .into_any_element()
    }

    /// A few named choices, the chosen one filled; they wrap when long.
    #[allow(clippy::too_many_arguments)]
    fn choice_row(
        &self,
        element: impl Into<SharedString>,
        label: impl Into<SharedString>,
        choices: Vec<(String, String)>,
        selected: &str,
        kind: Kind,
        id: &Option<String>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let element: SharedString = element.into();
        let label: SharedString = label.into();
        let value = choices
            .iter()
            .find(|(action, _)| action == selected)
            .map(|(_, name)| name.clone())
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .gap_2()
                    .text_size(px(12.))
                    .child(div().text_color(content(0.5)).child(label.clone()))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_color(content(1.))
                            .child(value),
                    ),
            )
            .child(
                div()
                    .id(element.clone())
                    .role(Role::RadioGroup)
                    .aria_label(label)
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .children(choices.into_iter().enumerate().map(|(ix, (action, name))| {
                        let chosen = action == selected;
                        let model = id.clone();
                        div()
                            .id(SharedString::from(format!("{element}-{ix}")))
                            .role(Role::RadioButton)
                            .aria_label(name.clone())
                            .aria_selected(chosen)
                            .max_w_full()
                            .px(px(10.))
                            .py(px(4.))
                            .rounded(px(6.))
                            .text_size(px(12.))
                            .truncate()
                            .map(|d| {
                                if chosen {
                                    d.bg(content(0.12)).text_color(content(1.))
                                } else {
                                    d.bg(content(0.04)).text_color(content(0.6))
                                }
                            })
                            .when(enabled && !chosen, |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.text_color(content(1.)))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.option(kind, model.clone(), action.clone(), cx)
                                    }))
                            })
                            .child(name)
                    })),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn model(id: &str, efforts: &[&str]) -> Model {
        Model {
            id: id.into(),
            name: id.into(),
            alias_target: None,
            efforts: efforts.iter().map(|e| (*e).into()).collect(),
            default_effort: efforts.first().map(|e| (*e).into()),
            fast: false,
            thinking: false,
            default: false,
            tiers: Vec::new(),
            controls: Vec::new(),
        }
    }

    #[derive(Debug, PartialEq)]
    enum Event {
        Option(&'static str, Option<String>, String),
        Picked(&'static str, Option<String>),
    }
    type Events = std::rc::Rc<std::cell::RefCell<Vec<Event>>>;

    /// A Codex session on `gpt-a` at medium, with Opus saved as favorite 1
    /// at high; only Codex and Claude are offered.
    fn picker(
        cx: &mut TestAppContext,
    ) -> (gpui::WindowHandle<ModelList>, Events, Vec<Subscription>) {
        cx.update(|cx| {
            cx.set_global(crate::settings::Preferences {
                favorite_models: vec![ModelChoice {
                    provider: "claude".into(),
                    model: Some("opus".into()),
                }],
                favorite_efforts: HashMap::from([("claude/opus".to_owned(), "high".to_owned())]),
                ..Default::default()
            })
        });
        let hidden: Vec<String> = Kind::ALL
            .into_iter()
            .filter(|k| !matches!(k, Kind::Codex | Kind::Claude))
            .map(|k| k.id().to_owned())
            .collect();
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|cx| {
                    let mut list = ModelList::new_for(
                        Kind::Codex,
                        Some("gpt-a".into()),
                        "/work",
                        Default::default(),
                        cx,
                    )
                    .hide_providers(hidden);
                    list.catalogs = vec![
                        (
                            Kind::Codex,
                            vec![
                                model("gpt-a", &["low", "medium", "high"]),
                                model("gpt-b", &[]),
                            ],
                        ),
                        (
                            Kind::Claude,
                            vec![model("opus", &["low", "medium", "high"])],
                        ),
                    ];
                    list.sections(
                        PanelState {
                            effort: Some("medium".into()),
                            ..Default::default()
                        },
                        cx,
                    )
                })
            })
            .unwrap()
        });
        let events = Events::default();
        let subscriptions = window
            .update(cx, |_, _, cx| {
                let entity = cx.entity();
                let (a, b) = (events.clone(), events.clone());
                vec![
                    cx.subscribe(&entity, move |_, _, e: &ModelOption, _| {
                        a.borrow_mut().push(Event::Option(
                            e.kind.id(),
                            e.model.clone(),
                            e.action.to_string(),
                        ))
                    }),
                    cx.subscribe(&entity, move |_, _, e: &ModelPicked, _| {
                        b.borrow_mut().push(Event::Picked(e.0.id(), e.1.clone()))
                    }),
                ]
            })
            .unwrap();
        (window, events, subscriptions)
    }

    fn click(visual: &mut VisualTestContext, selector: String) {
        let selector: &'static str = Box::leak(selector.into_boxed_str());
        visual.refresh().unwrap();
        let bounds = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("{selector} is rendered"));
        visual.simulate_click(bounds.center(), Default::default());
        visual.refresh().unwrap();
    }

    #[gpui::test]
    fn favorites_lead_with_their_effort_then_the_sessions_provider(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        window
            .update(cx, |list, _, cx| {
                let rows = list.section_rows(cx);
                let shown: Vec<_> = rows
                    .iter()
                    .map(|r| (r.section, r.entry.id.clone(), r.effort.clone(), r.number))
                    .collect();
                assert_eq!(
                    shown[0],
                    (
                        Section::Favorites,
                        Some("opus".into()),
                        Some("high".into()),
                        Some(1)
                    )
                );
                // The session's provider comes before the others.
                assert_eq!(shown[1].0, Section::Provider(Kind::Codex));
                let codex: Vec<_> = shown
                    .iter()
                    .filter(|r| r.0 == Section::Provider(Kind::Codex))
                    .map(|r| r.1.clone())
                    .collect();
                assert_eq!(codex, [None, Some("gpt-a".into()), Some("gpt-b".into())]);
                assert_eq!(shown.last().unwrap().0, Section::Provider(Kind::Claude));
                // It opens on the session's model.
                assert_eq!(rows[list.highlighted].entry.id.as_deref(), Some("gpt-a"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn the_rail_jumps_to_a_providers_models(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "model-rail-claude".into());
        window
            .update(&mut visual, |list, _, cx| {
                let row = &list.section_rows(cx)[list.highlighted];
                assert_eq!(row.section, Section::Provider(Kind::Claude));
                assert_eq!(row.entry.id, None, "its first row, the default");
            })
            .unwrap();
        click(&mut visual, "model-rail-favorites".into());
        window
            .update(&mut visual, |list, _, _| assert_eq!(list.highlighted, 0))
            .unwrap();
    }

    #[gpui::test]
    fn picking_a_favorite_brings_its_effort(cx: &mut TestAppContext) {
        let (window, events, _subs) = picker(cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "model-option-0".into());
        assert_eq!(
            *events.borrow(),
            [
                Event::Option("claude", Some("opus".into()), "effort:high".into()),
                Event::Picked("claude", Some("opus".into())),
            ]
        );
    }

    #[gpui::test]
    fn the_panel_sets_the_highlighted_models_effort_and_remembers_a_favorites(
        cx: &mut TestAppContext,
    ) {
        let (window, events, _subs) = picker(cx);
        window
            .update(cx, |list, _, cx| {
                list.highlighted = 0;
                cx.notify();
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "model-effort-0".into());
        assert_eq!(
            *events.borrow(),
            [Event::Option(
                "claude",
                Some("opus".into()),
                "effort:low".into()
            )]
        );
        cx.update(|cx| {
            let prefs = cx.global::<crate::settings::Preferences>();
            assert_eq!(
                prefs
                    .favorite_efforts
                    .get("claude/opus")
                    .map(String::as_str),
                Some("low")
            );
        });
    }

    #[gpui::test]
    fn a_new_favorite_keeps_the_effort_in_use(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        // The panel shows the session's model, gpt-a at medium.
        click(&mut visual, "model-favorite".into());
        cx.update(|cx| {
            let prefs = cx.global::<crate::settings::Preferences>();
            assert_eq!(prefs.favorite_models.len(), 2);
            assert_eq!(
                prefs
                    .favorite_efforts
                    .get("codex/gpt-a")
                    .map(String::as_str),
                Some("medium")
            );
            assert_eq!(
                favorite_choice(1, cx),
                Some((Kind::Codex, Some("gpt-a".into()), Some("medium".into())))
            );
        });
        // A second press removes it and its effort.
        click(&mut visual, "model-favorite".into());
        cx.update(|cx| {
            let prefs = cx.global::<crate::settings::Preferences>();
            assert_eq!(prefs.favorite_models.len(), 1);
            assert!(!prefs.favorite_efforts.contains_key("codex/gpt-a"));
        });
    }
}
