//! The composer's model picker, after Capy 0.9's: a compact list with
//! favorites first, each at the reasoning effort it opens at and numbered
//! for ⌥1–⌥9, then every model grouped by who makes it, or by the agent that
//! runs it with "By provider". Hovering a model opens a card beside the list
//! with its options: reasoning on a dithered slider, fast mode, thinking,
//! service tier, the provider's own controls, the account it runs on, and
//! whether it is a favorite. Changing one switches to that model and applies
//! the option through the composer's own settings actions (`effort:`,
//! `fast:on`, `usage-account:` …), so the picker holds no settings logic of
//! its own.
use super::*;
use crate::popover::{Anchor, AnchorSlot};
use crate::settings::ModelChoice;
use crate::visuals::dither;
use std::collections::HashMap;

const LIST_WIDTH: f32 = 320.;
const CARD_WIDTH: f32 = 288.;
pub(crate) const PICKER_WIDTH: f32 = LIST_WIDTH;
pub(crate) const PICKER_HEIGHT: f32 = 400.;
/// How long the card stays after the pointer leaves the list and the card,
/// so the pointer can cross the gap between them.
const CARD_HIDE_DELAY: std::time::Duration = std::time::Duration::from_millis(180);

/// The hover card beside the list.
#[derive(Default)]
pub(super) struct Card {
    pub(super) visible: bool,
    over_list: bool,
    over_card: bool,
    hide: Option<gpui::Task<()>>,
    /// The list's bounds, which the card opens beside.
    root: AnchorSlot,
    /// The card's bounds, so a press on it does not close the menu around
    /// the list.
    region: AnchorSlot,
    /// The reasoning slider's bounds, and its knob while it is dragged.
    track: AnchorSlot,
    drag: Option<EffortDrag>,
}

/// The reasoning knob under the pointer: where it is, in window
/// coordinates, and what letting go applies to.
pub(super) struct EffortDrag {
    x: f32,
    kind: Kind,
    model: Option<String>,
    efforts: Vec<String>,
}

/// Whether the list groups models by the agent that runs them rather than
/// by who makes them.
fn by_provider(cx: &gpui::App) -> bool {
    cx.try_global::<crate::settings::Preferences>()
        .is_some_and(|prefs| prefs.model_picker_by_provider)
}

/// The mark shown beside a maker's name, where an agent's mark is theirs.
fn maker_mark(maker: &str) -> Option<&'static str> {
    Some(match maker {
        "Anthropic" => "claude",
        "OpenAI" => "codex",
        "Google" => "antigravity",
        "xAI" => "grok",
        "Moonshot" => "kimi",
        "Cognition" => "devin",
        "Cursor" => "cursor",
        _ => return None,
    })
}

/// Whether a cell centred at (x, y) lies inside a box of `size` whose
/// corners in `round` (top-left, top-right, bottom-right, bottom-left) are
/// rounded by `radius`.
fn inside_corners(x: f32, y: f32, w: f32, h: f32, radius: f32, round: [bool; 4]) -> bool {
    let corners = [
        (radius, radius),
        (w - radius, radius),
        (w - radius, h - radius),
        (radius, h - radius),
    ];
    corners.iter().zip(round).all(|(&(cx, cy), round)| {
        let beyond_x = if cx < w / 2. { x < cx } else { x > cx };
        let beyond_y = if cy < h / 2. { y < cy } else { y > cy };
        !(round && beyond_x && beyond_y) || (x - cx).hypot(y - cy) <= radius
    })
}

/// Crisp dither cells: `CELL` squares on a `PITCH` grid with a gap between,
/// snapped to device pixels so they stay sharp, as Capy draws them.
const CELL: f32 = 3.;
const PITCH: f32 = 4.;

fn snap(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

/// Paints the lit cells of a `w` × `h` area at `origin`; `ink` gets each
/// cell's centre relative to the origin and its grid position.
fn crisp_cells(
    window: &mut Window,
    origin: gpui::Point<gpui::Pixels>,
    w: f32,
    h: f32,
    mut ink: impl FnMut(f32, f32, i32, i32) -> Option<gpui::Hsla>,
) {
    let scale = window.scale_factor();
    let (left, top) = (
        snap(f32::from(origin.x), scale),
        snap(f32::from(origin.y), scale),
    );
    let (cols, rows) = ((w / PITCH).floor() as i32, (h / PITCH).floor() as i32);
    for row in 0..rows {
        for col in 0..cols {
            let (x, y) = (col as f32 * PITCH, row as f32 * PITCH);
            let Some(color) = ink(x + CELL / 2., y + CELL / 2., col, row) else {
                continue;
            };
            window.paint_quad(gpui::fill(
                gpui::Bounds::new(
                    gpui::point(px(snap(left + x, scale)), px(snap(top + y, scale))),
                    gpui::size(px(CELL), px(CELL)),
                ),
                color,
            ));
        }
    }
}

/// When the corner's sweep started, shared by every card.
fn sweep_epoch() -> std::time::Instant {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(std::time::Instant::now)
}

/// The dithered glow in the card's top right corner: dense at the corner,
/// thinning toward the name. At the highest effort it moves: bands sweep
/// from the corner down and away, diagonally, inside the same area.
fn corner_dither(moving: bool) -> impl IntoElement {
    const WIDTH: f32 = 160.;
    const HEIGHT: f32 = 64.;
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            let still = !moving || cx.reduce_motion();
            let phase = if still {
                0.
            } else {
                window.request_animation_frame();
                sweep_epoch().elapsed().as_secs_f32()
            };
            let ink = accent_color();
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            crisp_cells(window, bounds.origin, w, h, |x, y, col, row| {
                // The card's own corner, less its 1px rule.
                if !inside_corners(x, y, w, h, 11., [false, true, false, false]) {
                    return None;
                }
                let (dx, dy) = ((w - x) / w, y / h);
                let reach = (dx * dx * 0.7 + dy * dy).sqrt();
                let mut tone = (1. - reach).max(0.).powf(1.5);
                if !still {
                    // A diagonal wave travelling out of the corner.
                    let along = dx * 0.6 + dy * 0.4;
                    let wave = ((along * 7. - phase * 2.6) * std::f32::consts::TAU / 2.).sin();
                    tone = (tone * (0.7 + 0.45 * wave)).clamp(0., 1.);
                }
                if !dither::inked(col, row, tone) {
                    return None;
                }
                // Denser cells read darker, as in Capy's corner.
                let mut color = ink;
                color.a *= 0.35 + 0.55 * tone;
                Some(color)
            });
        },
    )
    .absolute()
    .top_0()
    .right_0()
    .w(px(WIDTH))
    .h(px(HEIGHT))
}

const TRACK_INSET: f32 = 3.;
const KNOB: f32 = 14.;

/// Where the knob's centre sits for step `ix` of `steps` on a track `w` wide:
/// its right edge at the end of the step's slot.
fn knob_centre(ix: usize, steps: usize, w: f32) -> f32 {
    let usable = w - TRACK_INSET * 2.;
    TRACK_INSET + usable * (ix + 1) as f32 / steps as f32 - KNOB / 2.
}

/// The step whose knob position is nearest `x`, on a track `w` wide.
fn nearest_step(x: f32, steps: usize, w: f32) -> usize {
    (0..steps)
        .min_by(|a, b| {
            let (a, b) = (
                (knob_centre(*a, steps, w) - x).abs(),
                (knob_centre(*b, steps, w) - x).abs(),
            );
            a.total_cmp(&b)
        })
        .unwrap_or(0)
}

/// The reasoning slider's paint: `steps` slots, the fill and knob on `at`
/// (or under the pointer at `drag`, relative to the track, while it is
/// dragged), and a tick on every slot past it.
fn effort_track(steps: usize, at: Option<usize>, drag: Option<f32>) -> gpui::Canvas<()> {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            if steps == 0 {
                return;
            }
            let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let accent = accent_color();
            let centre = match drag {
                Some(x) => {
                    Some(x.clamp(knob_centre(0, steps, w), knob_centre(steps - 1, steps, w)))
                }
                None => at.map(|at| knob_centre(at, steps, w)),
            };
            for ix in 0..steps {
                let x = knob_centre(ix, steps, w);
                if centre.is_some_and(|c| x <= c + 1.) {
                    continue;
                }
                window.paint_quad(gpui::fill(
                    gpui::Bounds::new(
                        bounds.origin + gpui::point(px(x.round()), px(h / 2. - 5.)),
                        gpui::size(px(1.), px(10.)),
                    ),
                    content(0.22),
                ));
            }
            let Some(centre) = centre else {
                return;
            };
            let fill_w = centre - TRACK_INSET;
            let fill_h = h - TRACK_INSET * 2.;
            let origin = bounds.origin + gpui::point(px(TRACK_INSET), px(TRACK_INSET));
            let mut under = accent;
            under.a *= 0.28;
            window.paint_quad(
                gpui::fill(
                    gpui::Bounds::new(origin, gpui::size(px(fill_w), px(fill_h))),
                    under,
                )
                .corner_radii(px(6.)),
            );
            // Solid from the left, then crisp dots thinning toward the knob.
            let solid = (fill_w * 0.3).max(0.);
            window.paint_quad(
                gpui::fill(
                    gpui::Bounds::new(origin, gpui::size(px(solid), px(fill_h))),
                    accent,
                )
                .corner_radii(gpui::Corners {
                    top_left: px(6.),
                    bottom_left: px(6.),
                    top_right: px(0.),
                    bottom_right: px(0.),
                }),
            );
            let dots = gpui::point(px(solid), px(0.));
            crisp_cells(
                window,
                origin + dots,
                fill_w - solid,
                fill_h,
                |x, _, col, row| {
                    let t = x / (fill_w - solid).max(1.);
                    let tone = 1. - t.powf(0.85) * 0.92;
                    dither::inked(col, row, tone).then_some(accent)
                },
            );
            let knob = centre - KNOB / 2.;
            window.paint_quad(gpui::quad(
                gpui::Bounds::new(
                    bounds.origin + gpui::point(px(knob), px(TRACK_INSET)),
                    gpui::size(px(KNOB), px(fill_h)),
                ),
                px(6.),
                gpui::white(),
                px(2.),
                accent,
                gpui::BorderStyle::Solid,
            ));
        },
    )
}

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
    /// Models by who makes them, across the agents that run them.
    Maker(&'static str),
    /// Each agent's own default, when grouping by maker.
    Defaults,
    Provider(Kind),
}

pub(super) struct SectionRow {
    pub entry: ModelEntry,
    pub section: Section,
    /// A favorite's saved effort.
    pub effort: Option<String>,
    /// A favorite's ⌥ number, 1–9.
    pub number: Option<usize>,
    /// The agent that runs it, where a section mixes agents.
    pub via: Option<SharedString>,
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

    /// The bounds the hover card reports, so the menu around the list
    /// treats a press on the card as inside.
    pub(crate) fn set_card_region(&mut self, region: AnchorSlot) {
        self.card.region = region;
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
                    via: None,
                });
            }
        }
        let mut kinds = self.visible_kinds();
        // The session's own provider leads.
        kinds.sort_by_key(|kind| *kind != self.provider);
        if by_provider(cx) {
            for kind in kinds {
                rows.extend(
                    self.kind_entries(kind, cx)
                        .into_iter()
                        .map(|entry| SectionRow {
                            entry,
                            section: Section::Provider(kind),
                            effort: None,
                            number: None,
                            via: None,
                        }),
                );
            }
            return rows;
        }
        // By maker, in the order they first appear from the session's agent
        // on; models no maker claims stay under their agent, and the
        // agents' defaults come last.
        let mut groups: Vec<(Section, Vec<ModelEntry>)> = Vec::new();
        for kind in kinds {
            for entry in self.kind_entries(kind, cx) {
                let section = match &entry.id {
                    None => Section::Defaults,
                    Some(id) => catalog::model_maker(id, &entry.name)
                        .map_or(Section::Provider(kind), Section::Maker),
                };
                match groups.iter_mut().find(|(s, _)| *s == section) {
                    Some((_, entries)) => entries.push(entry),
                    None => groups.push((section, vec![entry])),
                }
            }
        }
        groups.sort_by_key(|(section, _)| match section {
            Section::Maker(_) => 0,
            Section::Provider(_) => 1,
            _ => 2,
        });
        for (section, entries) in groups {
            let mixed = section == Section::Defaults
                || entries.iter().any(|entry| entry.kind != entries[0].kind);
            rows.extend(entries.into_iter().map(|entry| SectionRow {
                via: mixed.then(|| entry.kind.label().into()),
                entry,
                section,
                effort: None,
                number: None,
            }));
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
        let mut list: Vec<AnyElement> = Vec::new();
        let mut last: Option<Section> = None;
        for (ix, row) in rows.into_iter().enumerate() {
            if last != Some(row.section) {
                list.push(self.section_header(row.section, last.is_some(), cx));
                last = Some(row.section);
            }
            list.push(self.section_row(ix, row, cx));
        }
        if list.is_empty() {
            list.push(
                div()
                    .px_2()
                    .py_3()
                    .text_size(px(13.))
                    .text_color(content(0.5))
                    .child("No matching models")
                    .into_any_element(),
            );
        }
        let by_provider = by_provider(cx);
        let card = self.card.visible.then_some(highlighted).flatten().map(
            |(kind, id, name, enabled, effort)| {
                let this = cx.entity().downgrade();
                crate::popover::popover("model-card", Anchor::Slot(self.card.root.clone()))
                    .side(crate::popover::Side::Right)
                    .align(crate::popover::Align::Start)
                    .gap(12.)
                    .width(CARD_WIDTH)
                    .priority(41)
                    .track(self.card.region.clone())
                    .on_hover(move |hovered, _, cx| {
                        let _ =
                            this.update(cx, |this, cx| this.card_hover(None, Some(*hovered), cx));
                    })
                    .child(self.panel(kind, id, name, enabled, effort, cx))
            },
        );
        if card.is_none() {
            // A hidden card must not keep the menu open for presses where it was.
            self.card.region.set(None);
        }
        div()
            .id("model-picker")
            .role(Role::Dialog)
            .aria_label("Models")
            .relative()
            .w(px(LIST_WIDTH))
            .h(px(PICKER_HEIGHT))
            .flex()
            .flex_col()
            .child(self.card.root.probe())
            .capture_action(cx.listener(|this, _: &input::Up, _, cx| {
                this.step(-1, cx);
                this.show_card_for_keys(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Down, _, cx| {
                this.step(1, cx);
                this.show_card_for_keys(cx);
                cx.stop_propagation();
            }))
            .capture_action(cx.listener(|this, _: &input::Submit, _, cx| {
                this.pick(this.highlighted, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex_shrink_0()
                    .h(px(40.))
                    .pl_3()
                    .pr(px(6.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .border_b_1()
                    .border_color(content(0.07))
                    .text_color(content(1.))
                    .child(div().flex_1().min_w_0().child(self.search.clone()))
                    .child(
                        div()
                            .id("model-by-provider")
                            .debug_selector(|| "model-by-provider".into())
                            .role(Role::Switch)
                            .aria_label("Group by provider")
                            .aria_selected(by_provider)
                            .flex_shrink_0()
                            .h(px(26.))
                            .px(px(8.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .map(|d| {
                                if by_provider {
                                    d.bg(content(0.1)).text_color(content(0.95))
                                } else {
                                    d.text_color(content(0.5))
                                        .hover(|s| s.bg(content(0.06)).text_color(content(0.9)))
                                }
                            })
                            .child(icon("queue-list", 13.))
                            .child("By provider")
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_by_provider(cx))),
                    ),
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
                    .p_1()
                    .flex()
                    .flex_col()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        this.card_hover(Some(*hovered), None, cx)
                    }))
                    .children(list),
            )
            .children(card)
            .into_any_element()
    }

    /// The pointer entered or left the list (`list`) or the card (`card`).
    /// The card shows while either is under it, and waits a moment before it
    /// hides, so the pointer can cross the gap between them.
    fn card_hover(&mut self, list: Option<bool>, card: Option<bool>, cx: &mut Context<Self>) {
        let before = (self.card.over_list, self.card.over_card);
        if let Some(list) = list {
            self.card.over_list = list;
        }
        if let Some(card) = card {
            self.card.over_card = card;
        }
        if before == (self.card.over_list, self.card.over_card) {
            return;
        }
        if self.card.over_list || self.card.over_card {
            self.card.hide = None;
            if !self.card.visible {
                self.card.visible = true;
                cx.notify();
            }
            return;
        }
        if !self.card.visible || self.card.hide.is_some() {
            return;
        }
        self.card.hide = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(CARD_HIDE_DELAY).await;
            let _ = this.update(cx, |this, cx| {
                this.card.hide = None;
                if !this.card.over_list && !this.card.over_card {
                    this.card.visible = false;
                    cx.notify();
                }
            });
        }));
    }

    /// Arrow keys show the highlighted model's card without the pointer.
    fn show_card_for_keys(&mut self, cx: &mut Context<Self>) {
        self.card.hide = None;
        self.card.visible = true;
        cx.notify();
    }

    fn toggle_by_provider(&mut self, cx: &mut Context<Self>) {
        if !cx.has_global::<crate::settings::Preferences>() {
            return;
        }
        let at = self
            .section_rows(cx)
            .get(self.highlighted)
            .map(|row| (row.entry.kind, row.entry.id.clone()));
        cx.update_global::<crate::settings::Preferences, _>(|prefs, _| {
            prefs.model_picker_by_provider = !prefs.model_picker_by_provider;
        });
        // Stay on the same model in its new place.
        let rows = self.section_rows(cx);
        self.highlighted = at
            .and_then(|(kind, id)| {
                rows.iter().position(|row| {
                    row.section != Section::Favorites
                        && row.entry.kind == kind
                        && row.entry.id == id
                })
            })
            .unwrap_or_else(|| self.current_index(cx));
        self.list_scroll.scroll_to_top_of_item(0);
        cx.notify();
    }

    fn section_header(&self, section: Section, rule: bool, cx: &mut Context<Self>) -> AnyElement {
        let status = match section {
            Section::Provider(kind) if self.loading.contains(&kind) => Some(("Loading…", None)),
            Section::Provider(kind) => self
                .errors
                .iter()
                .find(|(k, _)| *k == kind)
                .map(|(_, error)| ("Retry", Some((kind, error.clone())))),
            Section::Maker(_) | Section::Favorites | Section::Defaults => None,
        };
        let mark = match section {
            Section::Provider(kind) => Some(kind.id()),
            Section::Maker(maker) => maker_mark(maker),
            Section::Favorites | Section::Defaults => None,
        };
        let label: SharedString = match section {
            Section::Favorites => "Favorites".into(),
            Section::Defaults => "Agent defaults".into(),
            Section::Maker(maker) => maker.into(),
            Section::Provider(kind) => kind.label().into(),
        };
        div()
            .flex_shrink_0()
            .h(px(28.))
            .when(rule, |d| d.mt_1())
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .text_size(px(12.))
            .text_color(content(0.45))
            .when_some(mark, |d, mark| {
                d.child(provider_icon(mark, 12.).opacity(0.6))
            })
            .child(label)
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
            })
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
            .h(px(30.))
            .px_2()
            .rounded(px(8.))
            .flex()
            .items_center()
            .gap(px(10.))
            .text_size(px(13.))
            .text_color(content(if entry.enabled { 1. } else { 0.3 }))
            .when(entry.enabled, |d| d.cursor_pointer())
            .when(highlighted, |d| d.bg(content(0.08)))
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
                    .when_some(row.via, |d, via| {
                        d.child(
                            div()
                                .flex_shrink_0()
                                .text_color(content(0.4))
                                .child(format!("· {via}")),
                        )
                    })
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
                d.child(icon("check-2", 14.).text_color(content(0.7)))
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
        // The highest effort (Max, Ultrathink…) sets the corner moving.
        let mut top_effort = false;
        if let Some(model) = &model {
            let effort = state
                .effort
                .clone()
                .or_else(|| model.default_effort.clone())
                .or_else(|| model.efforts.first().cloned());
            if !model.efforts.is_empty() {
                let shown = self.effort_shown(&model.efforts, effort.as_deref());
                top_effort = model.efforts.len() > 1 && shown == Some(model.efforts.len() - 1);
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
        let rule = || div().h(px(1.)).bg(content(0.07)).flex_shrink_0();
        let has_blocks = !blocks.is_empty();
        div()
            .id("model-options")
            .role(Role::Group)
            .aria_label("Model options")
            .relative()
            .flex()
            .flex_col()
            // Capy's card: the dithered glow in the corner, behind the name.
            .child(corner_dither(top_effort))
            .child(
                div()
                    .relative()
                    .px_3()
                    .pt_3()
                    .pb(px(10.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(provider_icon(kind.id(), 18.))
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
            .when(has_blocks, |d| {
                d.child(rule()).child(
                    div()
                        .relative()
                        .px_3()
                        .py(px(10.))
                        .flex()
                        .flex_col()
                        .gap_3()
                        .children(blocks),
                )
            })
            .child(rule())
            .child(
                div().p(px(6.)).child(
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
                            icon(if starred { "star-filled" } else { "add-01" }, 14.)
                                .text_color(content(if starred { 0.85 } else { 0.7 })),
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
                ),
            )
            .into_any_element()
    }

    /// The effort the slider shows: under a dragged knob, else `selected`.
    fn effort_shown(&self, efforts: &[String], selected: Option<&str>) -> Option<usize> {
        if let (Some(drag), Some(track)) = (&self.card.drag, self.card.track.get())
            && drag.efforts == efforts
        {
            let x = drag.x - f32::from(track.left());
            return Some(nearest_step(x, efforts.len(), f32::from(track.size.width)));
        }
        selected.and_then(|s| efforts.iter().position(|e| e == s))
    }

    /// The knob follows the pointer.
    fn move_effort_drag(&mut self, x: f32, cx: &mut Context<Self>) {
        if let Some(drag) = self.card.drag.as_mut()
            && drag.x != x
        {
            drag.x = x;
            cx.notify();
        }
    }

    /// Letting go: the knob settles on the nearest effort, which applies.
    fn end_effort_drag(&mut self, x: f32, cx: &mut Context<Self>) {
        let Some(drag) = self.card.drag.take() else {
            return;
        };
        let Some(track) = self.card.track.get() else {
            return cx.notify();
        };
        let ix = nearest_step(
            x - f32::from(track.left()),
            drag.efforts.len(),
            f32::from(track.size.width),
        );
        match drag.efforts.get(ix) {
            Some(effort) => self.option(drag.kind, drag.model, format!("effort:{effort}"), cx),
            None => cx.notify(),
        }
    }

    /// Reasoning as Capy's slider: one slot per effort, a dithered fill up
    /// to the knob on the chosen one, and a tick where each other one sits.
    /// Drag the knob, or press anywhere on the track.
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
        let shown = self.effort_shown(efforts, selected);
        let label = if kind == Kind::Codex {
            "Reasoning"
        } else {
            "Effort"
        };
        let steps = efforts.len();
        let track = self.card.track.clone();
        let dragging = self
            .card
            .drag
            .as_ref()
            .filter(|drag| drag.efforts == efforts)
            .map(|drag| drag.x);
        let relative = dragging.and_then(|x| Some(x - f32::from(track.get()?.left())));
        let painter = effort_track(steps, at, relative);
        let this = cx.entity().downgrade();
        // While dragging, the pointer is followed past the track's edges.
        let follow = gpui::canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                if dragging.is_none() {
                    return;
                }
                let moved = this.clone();
                window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble {
                        let x = f32::from(event.position.x);
                        let _ = moved.update(cx, |this, cx| this.move_effort_drag(x, cx));
                    }
                });
                let released = this.clone();
                window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, _, cx| {
                    if phase == gpui::DispatchPhase::Bubble {
                        let x = f32::from(event.position.x);
                        let _ = released.update(cx, |this, cx| this.end_effort_drag(x, cx));
                    }
                });
            },
        )
        .absolute()
        .size_0();
        let (drag_kind, drag_model, drag_efforts) = (kind, id.clone(), efforts.to_vec());
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(13.))
                    .child(div().text_color(content(0.5)).child(label))
                    .child(
                        div().text_color(content(1.)).child(
                            shown
                                .and_then(|ix| efforts.get(ix))
                                .map(|effort| crate::conversation::effort_name(effort))
                                .unwrap_or_default(),
                        ),
                    ),
            )
            .child(
                div()
                    .id("model-reasoning")
                    .debug_selector(|| "model-reasoning".into())
                    .role(Role::Slider)
                    .aria_label(label)
                    .relative()
                    .h(px(28.))
                    .rounded(px(8.))
                    .bg(content(0.06))
                    .child(track.probe())
                    .child(painter.absolute().inset_0())
                    .child(follow)
                    .child(div().absolute().inset_0().flex().children(
                        efforts.iter().enumerate().map(|(ix, effort)| {
                            div()
                                .id(("model-effort", ix))
                                .debug_selector(move || format!("model-effort-{ix}"))
                                .role(Role::RadioButton)
                                .aria_label(crate::conversation::effort_name(effort))
                                .aria_selected(at == Some(ix))
                                .flex_1()
                                .h_full()
                        }),
                    ))
                    .when(enabled, |d| {
                        d.cursor_pointer()
                            .on_mouse_down(
                                gpui::MouseButton::Left,
                                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                                    this.card.drag = Some(EffortDrag {
                                        x: f32::from(event.position.x),
                                        kind: drag_kind,
                                        model: drag_model.clone(),
                                        efforts: drag_efforts.clone(),
                                    });
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .on_mouse_up(
                                gpui::MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseUpEvent, _, cx| {
                                    this.end_effort_drag(f32::from(event.position.x), cx)
                                }),
                            )
                    }),
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
        let element: SharedString = element.into();
        let fast = element.as_ref() == "model-fast";
        let (model, action) = (id.clone(), action.to_owned());
        div()
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(13.))
            .text_color(content(0.9))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when(fast, |d| {
                        d.child(icon("gauge", 14.).text_color(content(0.6)))
                    })
                    .child(label.clone()),
            )
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
    fn favorites_lead_with_their_effort_then_models_by_maker(cx: &mut TestAppContext) {
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
                let sections: Vec<_> =
                    rows.iter()
                        .skip(1)
                        .map(|r| r.section)
                        .fold(Vec::new(), |mut all, section| {
                            if all.last() != Some(&section) {
                                all.push(section);
                            }
                            all
                        });
                // The session's agent's makers lead; each agent's default
                // comes last, named by the agent.
                assert_eq!(
                    sections,
                    [
                        Section::Maker("OpenAI"),
                        Section::Maker("Anthropic"),
                        Section::Defaults
                    ]
                );
                let defaults: Vec<_> = rows
                    .iter()
                    .filter(|r| r.section == Section::Defaults)
                    .map(|r| r.via.clone())
                    .collect();
                assert_eq!(
                    defaults,
                    [
                        Some(Kind::Codex.label().into()),
                        Some(Kind::Claude.label().into())
                    ]
                );
                // It opens on the session's model.
                assert_eq!(rows[list.highlighted].entry.id.as_deref(), Some("gpt-a"));
            })
            .unwrap();
    }

    #[gpui::test]
    fn by_provider_groups_by_the_agent_and_keeps_the_model(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        click(&mut visual, "model-by-provider".into());
        window
            .update(&mut visual, |list, _, cx| {
                let rows = list.section_rows(cx);
                let codex: Vec<_> = rows
                    .iter()
                    .filter(|r| r.section == Section::Provider(Kind::Codex))
                    .map(|r| r.entry.id.clone())
                    .collect();
                assert_eq!(codex, [None, Some("gpt-a".into()), Some("gpt-b".into())]);
                assert_eq!(
                    rows.last().unwrap().section,
                    Section::Provider(Kind::Claude)
                );
                assert_eq!(rows[list.highlighted].entry.id.as_deref(), Some("gpt-a"));
            })
            .unwrap();
        cx.update(|cx| {
            assert!(
                cx.global::<crate::settings::Preferences>()
                    .model_picker_by_provider
            )
        });
    }

    #[gpui::test]
    fn the_options_card_opens_while_a_model_is_hovered(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        assert!(
            visual.debug_bounds("model-favorite").is_none(),
            "no card before the pointer comes"
        );
        let row = visual.debug_bounds("model-option-0").unwrap();
        visual.simulate_mouse_move(row.center(), None, Default::default());
        visual.refresh().unwrap();
        let card = visual
            .debug_bounds("model-favorite")
            .expect("the card opens on hover");
        assert!(card.left() > row.right(), "beside the list");
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
                list.card.visible = true;
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
    fn the_effort_knob_drags_and_settles_on_the_nearest_effort(cx: &mut TestAppContext) {
        let (window, events, _subs) = picker(cx);
        window
            .update(cx, |list, _, cx| {
                list.highlighted = 0;
                list.card.visible = true;
                cx.notify();
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        let low = visual.debug_bounds("model-effort-0").unwrap().center();
        let high = visual.debug_bounds("model-effort-2").unwrap().center();
        visual.simulate_mouse_down(low, gpui::MouseButton::Left, Default::default());
        visual.refresh().unwrap();
        // Halfway, the label already names the effort under the knob.
        visual.simulate_mouse_move(high, gpui::MouseButton::Left, Default::default());
        visual.refresh().unwrap();
        assert!(events.borrow().is_empty(), "nothing applies mid-drag");
        visual.simulate_mouse_up(high, gpui::MouseButton::Left, Default::default());
        visual.refresh().unwrap();
        assert_eq!(
            *events.borrow(),
            [Event::Option(
                "claude",
                Some("opus".into()),
                "effort:high".into()
            )]
        );
        window
            .update(&mut visual, |list, _, _| assert!(list.card.drag.is_none()))
            .unwrap();
    }

    #[test]
    fn the_knob_settles_on_the_nearest_step() {
        let w = 240.;
        assert_eq!(nearest_step(0., 4, w), 0);
        assert_eq!(nearest_step(w, 4, w), 3);
        assert_eq!(nearest_step(knob_centre(2, 4, w) + 3., 4, w), 2);
    }

    #[gpui::test]
    fn a_new_favorite_keeps_the_effort_in_use(cx: &mut TestAppContext) {
        let (window, _, _subs) = picker(cx);
        window
            .update(cx, |list, _, cx| {
                list.card.visible = true;
                cx.notify();
            })
            .unwrap();
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        // The card shows the session's model, gpt-a at medium.
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
