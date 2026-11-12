//! Environment (IVY's «Entorno» card): a frame that floats beside the chat in
//! view and says where it works, its Git branch and changes, and how full
//! its context window is. It sits in the chat's margin when the margin fits
//! it, pushes the chat left when the chat still fits beside it, and covers
//! the chat in a narrow pane. Cmd/Ctrl+I, the composer's Environment button
//! or a click on the context ring toggles it; Esc closes it.
use super::*;
use crate::pane_motion::PaneSlide;
use crate::popover::AnchorSlot;
use crate::right_panel::{DIFF_ADD, DIFF_DEL, format_integer};
use gpui::{App, FontWeight, KeyBinding, actions, rgb};
use std::collections::HashSet;

actions!(environment, [Toggle]);

pub(crate) fn bind(cx: &mut App) {
    let m = if cfg!(target_os = "macos") {
        "cmd"
    } else {
        "ctrl"
    };
    cx.bind_keys([KeyBinding::new(&format!("{m}-i"), Toggle, Some("Session"))]);
}

/// The frame's width, as IVY's SESSION_WIDTH.
pub(super) const WIDTH: f32 = 300.;
/// Space between the frame and the chat.
const GAP: f32 = 16.;
/// The narrowest the chat gets beside the frame.
const CHAT_MIN: f32 = 520.;
/// The narrowest the frame gets beside the chat.
const MIN: f32 = 220.;
/// The frame's distance from the pane's edges.
const EDGE: f32 = 8.;
/// Context fill that turns the bar amber, then red.
const WARN: f32 = 0.8;
const FULL: f32 = 0.95;

/// Where the frame goes in a pane `available` wide whose chat column is
/// `column` wide: its distance from the right edge, the room it takes from
/// the chat (pushing it left) and its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Room {
    pub right: f32,
    pub inset: f32,
    pub width: f32,
}

/// IVY's `sessionRoom`: in the margin when it fits, beside the chat while
/// the chat keeps `CHAT_MIN`, over the chat otherwise, so the button always
/// shows something.
pub(super) fn room(available: f32, column: f32) -> Room {
    let margin = (available - column) / 2.;
    if margin >= WIDTH + GAP + EDGE {
        return Room {
            right: margin - WIDTH - GAP,
            inset: 0.,
            width: WIDTH,
        };
    }
    let width = WIDTH.min(available - CHAT_MIN - GAP);
    if width >= MIN {
        return Room {
            right: EDGE,
            inset: width + GAP,
            width,
        };
    }
    Room {
        right: EDGE,
        inset: 0.,
        width: WIDTH.min((available - 2. * EDGE).max(0.)),
    }
}

/// The bar's colour for a context window this full.
fn fill_color(ratio: f32) -> gpui::Hsla {
    if ratio >= FULL {
        rgb(crate::right_panel::RED_400).into()
    } else if ratio >= WARN {
        rgb(crate::right_panel::AMBER_400).into()
    } else {
        accent_color()
    }
}

#[derive(Default)]
pub(super) struct Environment {
    pub open: bool,
    /// Openness from 0 to 1 while it slides in or out.
    slide: Option<PaneSlide>,
    /// The pane's bounds, for the room the frame takes.
    pane: AnchorSlot,
    folded: HashSet<&'static str>,
}

impl Environment {
    /// How open the frame is now, asking for frames until it settles.
    fn openness(&mut self, window: &mut Window, reduce_motion: bool) -> f32 {
        let target = if self.open { 1. } else { 0. };
        if reduce_motion {
            self.slide = None;
            return target;
        }
        let Some(slide) = &self.slide else {
            return target;
        };
        let (value, done) = slide.at(std::time::Instant::now());
        if done {
            self.slide = None;
        } else {
            window.request_animation_frame();
        }
        value.clamp(0., 1.)
    }
}

impl Conversation {
    pub(super) fn toggle_environment(&mut self, cx: &mut Context<Self>) {
        let env = &mut self.environment;
        let now = std::time::Instant::now();
        let from = match &env.slide {
            Some(slide) => slide.at(now).0,
            None if env.open => 1.,
            None => 0.,
        };
        env.open = !env.open;
        let to = if env.open { 1. } else { 0. };
        env.slide = Some(match &env.slide {
            Some(slide) => slide.retarget(to, now),
            None => PaneSlide::new(from, to, WIDTH),
        });
        cx.notify();
    }

    /// Esc closes an open frame; true when it did.
    pub(super) fn close_environment(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.environment.open {
            return false;
        }
        self.toggle_environment(cx);
        true
    }

    /// The composer bar's button that opens the frame.
    pub(super) fn environment_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.environment.open;
        icon_button(
            "environment-toggle",
            if open {
                "Hide environment"
            } else {
                "Show environment"
            },
            "sidebar-right",
        )
        .aria_expanded(open)
        .size(px(22.))
        .rounded_md()
        .when(open, |d| d.bg(content(0.08)))
        .on_click(cx.listener(|this, _, _, cx| this.toggle_environment(cx)))
        .into_any_element()
    }

    /// The pane's probe, the room the chat gives the frame (its right
    /// padding) and the frame itself, while it shows.
    pub(super) fn environment_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (AnyElement, f32, Option<AnyElement>) {
        let probe = self.environment.pane.layout_probe().into_any_element();
        let openness = self.environment.openness(window, cx.reduce_motion());
        let Some(pane) = self.environment.pane.get() else {
            return (probe, 0., None);
        };
        if openness <= 0. {
            return (probe, 0., None);
        }
        let available = f32::from(pane.size.width);
        let room = room(available, COLUMN);
        let height = (f32::from(pane.size.height) - 2. * EDGE).max(0.);
        // Slides in from the pane's edge, fading as it goes.
        let offset = (1. - openness) * (room.width + room.right);
        let frame = div()
            .id("environment")
            .debug_selector(|| "environment".into())
            .role(Role::Complementary)
            .aria_label("Environment")
            .absolute()
            .top(px(EDGE))
            .right(px(room.right - offset))
            .w(px(room.width))
            .max_h(px(height))
            .opacity(openness)
            .overflow_y_scroll()
            .rounded(px(12.))
            .border_1()
            .border_color(content(0.08))
            .bg(rail_color())
            .shadow_lg()
            .text_size(px(12.))
            .line_height(px(16.))
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.environment_header(cx))
            .child(self.workspace_section(cx))
            .children(self.version_section(cx))
            .child(self.context_section(cx))
            .into_any_element();
        (probe, room.inset * openness, Some(frame))
    }

    fn environment_header(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .h(px(36.))
            .pl_3()
            .pr_1p5()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(content(0.9))
                    .child("Environment"),
            )
            .child(
                icon_button("environment-close", "Close environment", "x")
                    .size(px(22.))
                    .rounded_md()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.close_environment(cx);
                    })),
            )
            .into_any_element()
    }

    /// One part of the frame under its title; folded, the title carries
    /// `summary`.
    fn env_section(
        &self,
        id: &'static str,
        name: &'static str,
        summary: Option<AnyElement>,
        body: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let folded = self.environment.folded.contains(id);
        div()
            .border_t_1()
            .border_color(content(0.06))
            .px_1p5()
            .py_1p5()
            .child(
                div()
                    .id(id)
                    .debug_selector(move || format!("environment-{id}"))
                    .role(Role::Button)
                    .aria_label(name)
                    .aria_expanded(!folded)
                    .h(px(26.))
                    .px_1p5()
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .cursor_pointer()
                    .text_color(content(0.55))
                    .hover(|s| s.bg(content(0.05)).text_color(content(0.85)))
                    .child(
                        icon(
                            if folded {
                                "chevron-right"
                            } else {
                                "chevron-down"
                            },
                            12.,
                        )
                        .flex_shrink_0(),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .child(name),
                    )
                    .when(folded, |d| {
                        d.children(summary.map(|summary| {
                            div()
                                .ml_auto()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .child(summary)
                        }))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let folded = &mut this.environment.folded;
                        if !folded.remove(id) {
                            folded.insert(id);
                        }
                        cx.notify();
                    })),
            )
            .when(!folded, |d| {
                d.child(div().pt_0p5().flex().flex_col().children(body))
            })
            .into_any_element()
    }

    /// A row of the frame: an icon, a label, an optional trailing part, and
    /// a click when it does something.
    fn env_row(
        id: &'static str,
        glyph: &'static str,
        label: String,
        trailing: Option<AnyElement>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .h(px(30.))
            .px_1p5()
            .rounded_lg()
            .flex()
            .items_center()
            .gap_2()
            .text_color(content(0.8))
            .child(icon(glyph, 14.).flex_shrink_0().text_color(content(0.45)))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .children(trailing)
    }

    fn workspace_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let dir = self.execution_cwd().to_owned();
        let linked =
            self.git.as_ref().is_some_and(|git| git.linked) || self.settings.worktree_cwd.is_some();
        let label = if linked { "Worktree" } else { "Local folder" };
        let reveal = if cfg!(target_os = "macos") {
            "Show in Finder"
        } else {
            "Show in Explorer"
        };
        let place = Self::env_row(
            "environment-place",
            if linked { "folder-tree" } else { "folder" },
            label.into(),
            None,
        )
        .role(Role::Label)
        .aria_label(format!("{label}: {dir}"))
        .text_color(content(0.6))
        .tooltip(crate::theme::chrome_tip(dir.clone(), None));
        let open = Self::env_row("environment-reveal", "folder-open", reveal.into(), None)
            .role(Role::Button)
            .aria_label(reveal)
            .cursor_pointer()
            .hover(|s| s.bg(content(0.05)))
            .on_click(cx.listener(move |_, _, _, cx| {
                let path = std::path::PathBuf::from(&dir);
                cx.background_executor()
                    .spawn(async move {
                        let _ = monocode_core::files::reveal_path(&path);
                    })
                    .detach();
            }));
        self.env_section(
            "workspace",
            "Workspace",
            Some(
                div()
                    .truncate()
                    .text_size(px(11.))
                    .text_color(content(0.4))
                    .child(label)
                    .into_any_element(),
            ),
            vec![place.into_any_element(), open.into_any_element()],
            cx,
        )
    }

    /// The branch, which opens the branch picker, and the changes, which
    /// open the diff. Only in a Git folder.
    fn version_section(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let branch = self
            .branch
            .clone()
            .or_else(|| self.git.as_ref().and_then(|git| git.current.clone()))?;
        let detached = self.git.as_ref().is_some_and(|git| git.detached);
        let enabled = !self.busy && !self.preparing_worktree && self.git.is_some();
        let label = if detached {
            format!("detached {branch}")
        } else {
            branch.clone()
        };
        let branch_row = Self::env_row(
            "environment-branch",
            "git-branch",
            label.clone(),
            Some(
                icon("chevron-down", 12.)
                    .flex_shrink_0()
                    .text_color(content(0.4))
                    .into_any_element(),
            ),
        )
        .role(Role::Button)
        .aria_label(format!("Branch {label}"))
        .when(!enabled, |d| d.opacity(0.5))
        .when(enabled, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(content(0.05)))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.open_git(GitMode::Branch, window, cx);
                }))
        });
        let totals = self.change_totals(cx);
        let changes = match totals {
            Some((files, additions, deletions)) => Self::env_row(
                "environment-changes",
                "file-diff",
                format!(
                    "{files} changed {}",
                    if files == 1 { "file" } else { "files" }
                ),
                Some(diff_counts(additions, deletions)),
            )
            .role(Role::Button)
            .aria_label(format!(
                "Changes: {files} files, {additions} additions, {deletions} deletions"
            ))
            .cursor_pointer()
            .hover(|s| s.bg(content(0.05)))
            .on_click(cx.listener(|this, _, _, cx| {
                let (session, cwd) = this.review.read(cx).session();
                cx.emit(OpenSessionChanges(crate::review::ReviewTab::Session {
                    cwd: cwd.to_owned(),
                    session,
                    path: None,
                }));
            })),
            None => Self::env_row(
                "environment-changes",
                "file-diff",
                "No changes yet".into(),
                None,
            )
            .role(Role::Label)
            .text_color(content(0.45)),
        };
        let summary = div()
            .min_w_0()
            .truncate()
            .text_size(px(11.))
            .text_color(content(0.4))
            .child(branch)
            .into_any_element();
        Some(self.env_section(
            "version",
            "Version control",
            Some(summary),
            vec![branch_row.into_any_element(), changes.into_any_element()],
            cx,
        ))
    }

    /// How full the context window is: the tokens, the share and a bar that
    /// turns amber, then red; and compacting it.
    fn context_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let ratio = self.context.and_then(|usage| usage.ratio());
        let percent = ratio.map(|r| format!("{:.0}%", r * 100.));
        let mut body = Vec::new();
        match self.context {
            Some(usage) => {
                let used = monocode_core::context::format_tokens(usage.used);
                let of = usage
                    .window
                    .map(|w| format!("of {} tokens", monocode_core::context::format_tokens(w)))
                    .unwrap_or_else(|| "tokens".into());
                body.push(
                    div()
                        .px_1p5()
                        .pt_1()
                        .flex()
                        .items_baseline()
                        .gap_1p5()
                        .child(
                            div()
                                .text_size(px(20.))
                                .line_height(px(24.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .font_features(crate::right_panel::tabular())
                                .text_color(content(0.95))
                                .child(used),
                        )
                        .child(div().text_color(content(0.45)).child(of))
                        .children(percent.clone().map(|p| {
                            div()
                                .ml_auto()
                                .font_features(crate::right_panel::tabular())
                                .text_color(ratio.map_or(content(0.6), fill_color))
                                .child(p)
                        }))
                        .into_any_element(),
                );
                if let Some(ratio) = ratio {
                    body.push(
                        div()
                            .id("environment-context-bar")
                            .debug_selector(|| "environment-context-bar".into())
                            .role(Role::ProgressIndicator)
                            .aria_label(format!("{:.0}% of the context window used", ratio * 100.))
                            .mx_1p5()
                            .my_2()
                            .h(px(4.))
                            .rounded_full()
                            .bg(content(0.08))
                            .child(
                                div()
                                    .h_full()
                                    .rounded_full()
                                    .w(gpui::relative(ratio.clamp(0., 1.)))
                                    .bg(fill_color(ratio)),
                            )
                            .into_any_element(),
                    );
                }
            }
            None => body.push(
                div()
                    .px_1p5()
                    .py_1()
                    .text_color(content(0.45))
                    .child("No context used yet")
                    .into_any_element(),
            ),
        }
        if self.context.is_some() && !self.busy && !self.is_remote() {
            body.push(
                Self::env_row(
                    "environment-compact",
                    "gauge",
                    "Compact the conversation".into(),
                    None,
                )
                .role(Role::Button)
                .aria_label("Compact the conversation")
                .cursor_pointer()
                .hover(|s| s.bg(content(0.05)))
                .on_click(cx.listener(|this, _, _, cx| this.compact_context(cx)))
                .into_any_element(),
            );
        }
        let summary = percent.map(|p| {
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .w(px(28.))
                        .h(px(3.))
                        .rounded_full()
                        .bg(content(0.08))
                        .child(
                            div()
                                .h_full()
                                .rounded_full()
                                .w(gpui::relative(ratio.unwrap_or(0.).clamp(0., 1.)))
                                .bg(fill_color(ratio.unwrap_or(0.))),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .font_features(crate::right_panel::tabular())
                        .text_color(content(0.45))
                        .child(p),
                )
                .into_any_element()
        });
        self.env_section("context", "Context", summary, body, cx)
    }
}

/// "+12 −3" in the diff colours.
fn diff_counts(additions: i64, deletions: i64) -> AnyElement {
    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_1()
        .font_family(crate::fonts::mono())
        .text_size(px(11.))
        .font_features(crate::right_panel::tabular())
        .child(
            div()
                .text_color(rgb(DIFF_ADD))
                .child(format!("+{}", format_integer(additions))),
        )
        .child(
            div()
                .text_color(rgb(DIFF_DEL))
                .child(format!("−{}", format_integer(deletions))),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{
        Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowOptions, point, size,
    };
    use monocode_core::store::Store;
    use std::sync::Mutex;

    #[gpui::test]
    fn the_shortcut_opens_the_frame_beside_a_chat_that_moves_left_and_esc_closes_it(
        cx: &mut TestAppContext,
    ) {
        let store = Store::open(Path::new(":memory:")).unwrap();
        let summary = store
            .create("codex", "/missing/environment-project")
            .unwrap();
        let window = cx.update(|cx| {
            input::bind(cx);
            bind(cx);
            cx.bind_keys([KeyBinding::new("escape", crate::shell::DismissMenu, None)]);
            cx.set_reduce_motion(true);
            cx.open_window(
                WindowOptions {
                    // Too narrow for the margin, wide enough to keep the chat.
                    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                        point(px(0.), px(0.)),
                        size(px(1100.), px(700.)),
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    cx.new(|cx| {
                        let mut view = Conversation::new(summary, Arc::new(Mutex::new(store)), cx);
                        view.loaded = true;
                        view.summary.has_messages = true;
                        window.focus(&view.input.focus_handle(cx), cx);
                        view
                    })
                },
            )
            .unwrap()
        });
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.refresh().unwrap();
        visual.run_until_parked();
        assert!(visual.debug_bounds("environment").is_none());
        let before = visual.debug_bounds("composer-toolbar").unwrap();

        visual.simulate_keystrokes(if cfg!(target_os = "macos") {
            "cmd-i"
        } else {
            "ctrl-i"
        });
        visual.run_until_parked();
        visual.refresh().unwrap();
        let frame = visual.debug_bounds("environment").expect("the frame opens");
        assert_eq!(frame.size.width, px(WIDTH));
        assert!(visual.debug_bounds("environment-workspace").is_some());
        assert!(visual.debug_bounds("environment-context").is_some());
        let after = visual.debug_bounds("composer-toolbar").unwrap();
        assert!(
            after.right() <= frame.left() - px(GAP) + px(1.),
            "the chat moves left of the frame: {after:?} {frame:?}"
        );
        assert!(after.left() < before.left(), "{before:?} {after:?}");

        // A section folds under its title.
        let title = visual.debug_bounds("environment-workspace").unwrap();
        visual.simulate_click(title.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("environment-reveal").is_none());

        visual.simulate_keystrokes("escape");
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("environment").is_none());
        assert_eq!(visual.debug_bounds("composer-toolbar").unwrap(), before);

        // The composer's button opens it again.
        let button = visual.debug_bounds("environment-toggle").unwrap();
        visual.simulate_click(button.center(), Default::default());
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("environment").is_some());
    }

    #[test]
    fn the_frame_takes_the_margin_then_pushes_the_chat_then_covers_it() {
        // A wide pane: in the margin, the chat stays put.
        let wide = room(1600., COLUMN);
        assert_eq!(wide.inset, 0.);
        assert_eq!(wide.width, WIDTH);
        assert!(wide.right + wide.width + GAP <= (1600. - COLUMN) / 2. + 0.01);
        // Narrower: the chat moves left by the frame and the gap.
        let middle = room(1100., COLUMN);
        assert_eq!(
            (middle.right, middle.inset, middle.width),
            (EDGE, WIDTH + GAP, WIDTH)
        );
        // The frame narrows before giving up its room.
        let tight = room(CHAT_MIN + GAP + 240., COLUMN);
        assert_eq!(tight.width, 240.);
        assert_eq!(tight.inset, 240. + GAP);
        // A narrow pane: over the chat, never wider than the pane.
        let narrow = room(600., COLUMN);
        assert_eq!(narrow.inset, 0.);
        assert_eq!(narrow.width, WIDTH);
        assert_eq!(room(200., COLUMN).width, 200. - 2. * EDGE);
    }

    #[test]
    fn a_fuller_window_turns_amber_then_red() {
        assert_eq!(fill_color(0.5), accent_color());
        assert_eq!(fill_color(0.85), rgb(crate::right_panel::AMBER_400).into());
        assert_eq!(fill_color(0.97), rgb(crate::right_panel::RED_400).into());
    }
}
