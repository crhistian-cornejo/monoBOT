//! A subagent's face: a small Mono, as the user's own bots wear. Each one
//! keeps the species and color its name picks, so two subagents in the same
//! turn read apart at a glance, and it hops while it works.
use gpui::{AnyElement, div, prelude::*, px};
use monocode_core::mono::persona;
use monocode_core::{
    project_appearance::{COLORS, MASCOTS},
    tool::ToolState,
};

/// The face's side: the smallest a persona keeps its identity at.
const SIDE: f32 = 16.;
/// The failed tint the rows' text already uses.
const FAILED: u32 = 0xf87171;

/// JavaScript's `charCodeAt` hash (UTF-16 units, wrapping like `>>> 0`), as
/// the pixel mascots picked by: a subagent keeps the face it had.
fn hash(name: &str) -> usize {
    name.encode_utf16().fold(0u32, |hash, ch| {
        hash.wrapping_mul(131).wrapping_add(u32::from(ch))
    }) as usize
}

/// The persona face and color a subagent named `name` wears.
pub(crate) fn subagent_face(name: &str, state: &ToolState) -> (String, u32) {
    let hash = hash(name);
    let species = MASCOTS[hash % MASCOTS.len()];
    // The first color is the neutral grey; a face wears one of the others.
    let colors = &COLORS[1..];
    let color = match state {
        ToolState::Failed | ToolState::Cancelled => FAILED,
        _ => colors[(hash / MASCOTS.len()) % colors.len()],
    };
    (persona::key(species, None), color)
}

pub(crate) fn subagent_mascot(name: &str, state: &ToolState, active: bool) -> AnyElement {
    let (face, color) = subagent_face(name, state);
    div()
        .size(px(SIDE))
        .flex_shrink_0()
        // A finished one steps back beside the one still at work.
        .when(*state == ToolState::Completed, |d| d.opacity(0.7))
        .child(crate::monos::avatar::avatar(
            &face,
            color,
            if active { "working" } else { "idle" },
            SIDE,
            !active,
        ))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subagent_keeps_its_face_and_a_failed_one_turns_red() {
        let (face, color) = subagent_face("Inspect the notch hooks", &ToolState::Running);
        assert_eq!(
            subagent_face("Inspect the notch hooks", &ToolState::Completed),
            (face.clone(), color)
        );
        assert!(persona::parse(&face).is_some());
        assert_ne!(color, COLORS[0]);
        assert_eq!(
            subagent_face("Inspect the notch hooks", &ToolState::Failed).1,
            FAILED
        );
    }
}
