//! A subagent's status mark, beside its title: the working spinner while it
//! runs, the circled check once it is done and a red cross when it failed.
//! Mono faces belong to the user's own agents, so a provider's subagent
//! never wears one.
use crate::theme::*;
use gpui::{AnyElement, ElementId, Role, div, prelude::*, px, rgb};
use monocode_core::tool::ToolState;

/// The mark's side, the row's icon size.
const SIDE: f32 = 16.;
/// The failed tint the rows' text already uses.
const FAILED: u32 = 0xf87171;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    Working,
    /// Still marked running, but its turn is over: it will not finish now.
    Stopped,
    Done,
    Failed,
}

pub(crate) fn mark(state: &ToolState, active: bool) -> Mark {
    match state {
        ToolState::Running if active => Mark::Working,
        ToolState::Running | ToolState::Cancelled => Mark::Stopped,
        ToolState::Completed => Mark::Done,
        ToolState::Failed => Mark::Failed,
    }
}

impl Mark {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Mark::Working => "Working",
            Mark::Stopped => "Stopped",
            Mark::Done => "Done",
            Mark::Failed => "Failed",
        }
    }
}

/// `id` names the spinner's clock; reduced motion draws it still.
pub(crate) fn subagent_mark(
    id: impl Into<ElementId>,
    state: &ToolState,
    active: bool,
    reduce_motion: bool,
) -> AnyElement {
    let id: ElementId = id.into();
    let mark = mark(state, active);
    let glyph: AnyElement = match mark {
        Mark::Working if reduce_motion => crate::session_mark::spinner_at(0.3).into_any_element(),
        Mark::Working => crate::session_mark::spinner(id.clone()).into_any_element(),
        Mark::Stopped => icon("circle-dashed-hi", 14.)
            .text_color(content(0.35))
            .into_any_element(),
        Mark::Done => crate::session_mark::done_check(14.).into_any_element(),
        Mark::Failed => icon("circle-x", 14.)
            .text_color(rgb(FAILED))
            .into_any_element(),
    };
    div()
        .id(id)
        .size(px(SIDE))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .role(Role::Image)
        .aria_label(mark.label())
        .child(glyph)
        .into_any_element()
}

/// One mark for a group of subagents: working while any works, failed when
/// one failed and none works, done when all are done.
pub(crate) fn group_mark<'a>(marks: impl IntoIterator<Item = &'a Mark>) -> Mark {
    let marks: Vec<Mark> = marks.into_iter().copied().collect();
    if marks.contains(&Mark::Working) {
        Mark::Working
    } else if marks.contains(&Mark::Failed) {
        Mark::Failed
    } else if !marks.is_empty() && marks.iter().all(|mark| *mark == Mark::Done) {
        Mark::Done
    } else {
        Mark::Stopped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subagent_shows_a_check_once_done_and_a_cross_when_it_failed() {
        assert_eq!(mark(&ToolState::Running, true), Mark::Working);
        assert_eq!(mark(&ToolState::Running, false), Mark::Stopped);
        assert_eq!(mark(&ToolState::Completed, true), Mark::Done);
        assert_eq!(mark(&ToolState::Failed, false), Mark::Failed);
        assert_eq!(mark(&ToolState::Cancelled, false), Mark::Stopped);
    }

    #[test]
    fn a_group_works_while_one_works_and_is_done_when_all_are() {
        use Mark::*;
        assert_eq!(group_mark(&[Done, Working, Failed]), Working);
        assert_eq!(group_mark(&[Done, Failed]), Failed);
        assert_eq!(group_mark(&[Done, Done]), Done);
        assert_eq!(group_mark(&[Done, Stopped]), Stopped);
        assert_eq!(group_mark(&[]), Stopped);
    }
}
