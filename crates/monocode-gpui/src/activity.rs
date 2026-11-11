//! Direct presentation port of AgentTranscript's ActivityPhaseGroup and PhaseStep.
use gpui::{
    AnyElement, App, Bounds, ContentMask, Element, ElementId, GlobalElementId, InspectorElementId,
    LayoutId, Pixels, StyledText, Window, point, prelude::*,
};
use monocode_core::tool::ToolCall;

/// Shade a fixed glyph layout. Animated StyledText highlights change font-run
/// boundaries (and kerning/ligatures) as neighboring colors become equal.
pub(crate) fn shimmer(text: String, elapsed: f32) -> AnyElement {
    shimmer_in(text, elapsed, crate::theme::content)
}

/// [`shimmer`] in another ink, such as the notch's white on black.
pub(crate) fn shimmer_in(text: String, elapsed: f32, ink: fn(f32) -> gpui::Hsla) -> AnyElement {
    Shimmer {
        text: StyledText::new(text),
        elapsed,
        ink,
    }
    .into_any_element()
}

/// How lit a glyph is by the sweep of light: `x` is its place along the line
/// (0 to 1) and `elapsed` the seconds on the sweep's clock. One pass takes
/// 1.6 s. The text morph's shimmer shares it.
pub(crate) fn sweep(x: f32, elapsed: f32) -> f32 {
    let center = -0.25 + (elapsed / 1.6).fract() * 1.5;
    (1. - (x - center).abs() / (2. / 7.)).max(0.)
}

struct Shimmer {
    text: StyledText,
    elapsed: f32,
    ink: fn(f32) -> gpui::Hsla,
}
impl IntoElement for Shimmer {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for Shimmer {
    type RequestLayoutState = ();
    type PrepaintState = ();
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, None, window, cx)
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        // The sweep moves on the shared clock.
        crate::pulse::request(crate::pulse::Rate::Smooth, window, cx);
        self.text.prepaint(None, None, bounds, state, window, cx);
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let layout = self.text.layout();
        let line_height = layout.line_height();
        // Both callers use a single truncated line; retain StyledText's layout
        // so overflow, font fallback and the plain-text geometry stay identical.
        let lines = layout.line_layouts();
        window.paint_layer(bounds, |window| {
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                for wrapped in &lines {
                    let line = &wrapped.unwrapped_layout;
                    let baseline = (line_height - line.ascent - line.descent) / 2. + line.ascent;
                    let width = f32::from(line.width).max(1.);
                    for run in &line.runs {
                        for glyph in &run.glyphs {
                            let x = f32::from(glyph.position.x + line.font_size * 0.25) / width;
                            let sweep = sweep(x, self.elapsed);
                            let origin = bounds.origin
                                + point(glyph.position.x, baseline + glyph.position.y);
                            if glyph.is_emoji {
                                let _ = window.paint_emoji(
                                    origin,
                                    run.font_id,
                                    glyph.id,
                                    line.font_size,
                                );
                            } else {
                                let _ = window.paint_glyph(
                                    origin,
                                    run.font_id,
                                    glyph.id,
                                    line.font_size,
                                    (self.ink)(0.4 + 0.6 * sweep),
                                );
                            }
                        }
                    }
                }
            });
        });
    }
}

/// A work icon by name. `monocode` is the app's own logo, for its tools.
pub(crate) fn glyph(name: &'static str, side: f32) -> gpui::Div {
    if name == "monocode" {
        return gpui::div()
            .size(gpui::px(side))
            .flex_shrink_0()
            .child(gpui::img("monocode.png").size(gpui::px(side)));
    }
    gpui::div()
        .size(gpui::px(side))
        .flex_shrink_0()
        .child(crate::theme::icon(name, side))
}
pub(crate) fn file_icon(path: &str, side: f32) -> AnyElement {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    crate::file_icons::file_type_icon(name, false, false, side)
}
/// A group's header: one sentence for what it did (`core::tool::summary`)
/// and the icon of its busiest kind of step.
pub(crate) fn summary(tools: &[ToolCall], live: bool) -> (String, &'static str) {
    let summary = monocode_core::tool::summary::summarize(tools, live);
    (summary.text, summary.icon)
}
/// A step as it happens, for a running phase's one live line: "Reading
/// main.rs", "Running cargo test". A step with no verb of its own keeps its
/// caption as it is.
pub(crate) fn current_step(shown: &monocode_core::tool::ToolPresentation) -> String {
    let verb = match shown.action.as_str() {
        "Read" => "Reading",
        "Write" => "Writing",
        "Edit" => "Editing",
        "Find" | "Search" => "Searching",
        "List" => "Listing",
        "Run" => "Running",
        other => other,
    };
    let target = shown
        .command
        .as_deref()
        .filter(|_| shown.action == "Run" && shown.target.is_empty())
        .unwrap_or(&shown.target);
    [verb, target]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
/// CSS .zen-phase-step: left 6px spine, 8px quarter curve, 20px content inset.
pub(crate) fn rail(last: bool) -> impl IntoElement {
    rail_branch(last, true)
}
pub(crate) fn rail_branch(last: bool, branch: bool) -> impl IntoElement {
    rail_branch_at(last, branch, 0.)
}
/// The same rail for a step whose content sits `offset` lower, as the
/// sidebar's `.orchestration-tree-step` does with its 2px padding.
pub(crate) fn rail_branch_at(last: bool, branch: bool, offset: f32) -> impl IntoElement {
    rail_from(6., last, branch, offset)
}
/// The rail under a work phase's tile, dropping from the tile's centre.
pub(crate) fn tile_rail(last: bool) -> impl IntoElement {
    rail_from(TILE_RAIL_X, last, true, 0.)
}
/// Where a 1px line under an activity tile starts so it is centred on the
/// tile: half a pixel either side of its middle, which lands on whole device
/// pixels on a 2x screen.
pub(crate) const TILE_RAIL_X: f32 = crate::tab_tile::ACTIVITY_TILE.w / 2. - 0.5;
/// [`tile_rail`] for a step that may not branch off it.
pub(crate) fn tile_rail_branch(last: bool, branch: bool) -> impl IntoElement {
    rail_from(TILE_RAIL_X, last, branch, 0.)
}
/// Children of a [`tile_rail`] start where the group's title does: past the
/// tile and its gap.
pub(crate) const TILE_RAIL_INDENT: f32 = crate::tab_tile::ACTIVITY_TILE.w + 8.;
fn rail_from(left: f32, last: bool, branch: bool, offset: f32) -> impl IntoElement {
    let color = crate::theme::rail_color();
    gpui::div()
        .absolute()
        .top_0()
        .left(gpui::px(left))
        .w(gpui::px(9.))
        .h_full()
        .child(
            gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .w(gpui::px(1.))
                .map(|d| {
                    if last {
                        d.h(gpui::px(7. + offset))
                    } else {
                        d.h_full()
                    }
                })
                .bg(color),
        )
        .when(branch, |d| {
            d.child(
                gpui::svg()
                    .path("tool-branch")
                    .absolute()
                    .left_0()
                    .top(gpui::px(6. + offset))
                    .size(gpui::px(9.))
                    .text_color(color),
            )
        })
}

#[cfg(test)]
mod shimmer_tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext, canvas, div, px};
    use std::{cell::RefCell, rc::Rc};

    struct Preview {
        elapsed: f32,
        geometry: Rc<RefCell<Option<(Pixels, String)>>>,
    }
    impl Render for Preview {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let elapsed = self.elapsed;
            let geometry = self.geometry.clone();
            div()
                .text_size(px(14.))
                .line_height(px(20.))
                .truncate()
                .child(
                    canvas(
                        move |_, window, cx| {
                            let text = "GPT-6.1-Sol working for 29s · office AV café 漢字";
                            let shimmer = Shimmer {
                                text: StyledText::new(text),
                                elapsed,
                                ink: crate::theme::content,
                            };
                            let layout = shimmer.text.layout().clone();
                            let mut shimmer = shimmer.into_any_element();
                            let available = gpui::size(
                                gpui::AvailableSpace::Definite(px(220.)),
                                gpui::AvailableSpace::MinContent,
                            );
                            let measured = shimmer.layout_as_root(available, window, cx);
                            let plain = StyledText::new(text);
                            let plain_layout = plain.layout().clone();
                            let plain = plain
                                .into_any_element()
                                .layout_as_root(available, window, cx);
                            assert_eq!(measured, plain);
                            let glyphs = |layout: &gpui::TextLayout| {
                                layout
                                    .line_layouts()
                                    .iter()
                                    .map(|line| format!("{:?}", line.unwrapped_layout.runs))
                                    .collect::<String>()
                            };
                            assert_eq!(glyphs(&layout), glyphs(&plain_layout));
                            *geometry.borrow_mut() = Some((measured.width, glyphs(&layout)));
                        },
                        |_, _, _, _| {},
                    )
                    .w_full()
                    .h(px(20.)),
                )
        }
    }
    #[gpui::test]
    fn sweep_preserves_plain_text_kerning_ligatures_and_glyph_positions(cx: &mut TestAppContext) {
        let geometry = Rc::new(RefCell::new(None));
        let window = cx.update(|cx| {
            cx.open_window(Default::default(), |_, cx| {
                cx.new(|_| Preview {
                    elapsed: 0.,
                    geometry: geometry.clone(),
                })
            })
            .unwrap()
        });
        let mut first = None;
        for elapsed in [0., 0.2, 0.6, 1., 1.59, 1.6] {
            window
                .update(cx, |view, _, cx| {
                    view.elapsed = elapsed;
                    cx.notify();
                })
                .unwrap();
            cx.refresh().unwrap();
            cx.run_until_parked();
            let current = geometry.borrow().clone().expect("shimmer was measured");
            if let Some(first) = &first {
                assert_eq!(first, &current);
            } else {
                first = Some(current);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use monocode_core::tool::ToolCall;

    fn step(name: &str, input: serde_json::Value) -> String {
        let mut call = ToolCall::new("1", name);
        call.input = input.to_string();
        super::current_step(&call.presentation_in(Some("/repo")))
    }

    #[test]
    fn a_running_phase_names_its_step_as_it_happens() {
        assert_eq!(
            step(
                "Edit",
                serde_json::json!({"file_path": "/repo/src/main.rs"})
            ),
            "Editing src/main.rs"
        );
        assert!(
            step("Read", serde_json::json!({"file_path": "/repo/a.rs"})).starts_with("Reading")
        );
        assert!(step("Bash", serde_json::json!({"command": "cargo test"})).starts_with("Running"));
    }
}
