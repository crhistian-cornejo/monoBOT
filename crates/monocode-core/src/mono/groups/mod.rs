//! Groups of agents: a room the Principal leads, with folders opened to its
//! members, access requests, rounds where every member answers before the
//! lead, a seat for each member, @Name mentions, activity and learning.
//!
//! React's `src/features/groups/model` (and the group parts of `monos/model`
//! and `agent-app/model`) is the reference: same texts, same decisions. This
//! module is pure: it reads what it needs as arguments and returns new values,
//! and the caller keeps sessions, the roster and anything persisted.
pub mod activity;
pub mod app;
pub mod block;
pub mod board;
pub mod brief;
pub mod call;
pub mod group;
pub mod learning;
pub mod live;
pub mod mentions;
pub mod native;
pub mod project;
pub mod replies;
pub mod roster;
pub mod round;
pub mod seats;

pub use block::{Block, GroupMemberPost, TurnMetrics};
pub use brief::{Member, MemberKind};
pub use group::{
    parse, parse_entry, Access, AccessAnswer, AccessRequest, Folder, Group, GroupMono,
};
pub use roster::{read_roster, Roster};

/// A text's length as JavaScript counts it (UTF-16 units), so every limit
/// cuts where the desktop app cuts.
pub(crate) fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The longest start of `text` within `max` UTF-16 units, never splitting
/// a character (JavaScript's `slice(0, max)`).
pub(crate) fn js_prefix(text: &str, max: usize) -> &str {
    let mut units = 0;
    for (index, c) in text.char_indices() {
        units += c.len_utf16();
        if units > max {
            return &text[..index];
        }
    }
    text
}

/// Runs of whitespace as one space, trimmed (`replace(/\s+/g, " ").trim()`).
pub(crate) fn flat(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` UTF-16 units, the last one an ellipsis when cut.
pub(crate) fn clip(text: &str, max: usize) -> String {
    if js_len(text) > max {
        format!("{}…", js_prefix(text, max - 1))
    } else {
        text.to_string()
    }
}

/// The last `max` entries (`slice(-max)`).
pub(crate) fn keep_last<T>(mut items: Vec<T>, max: usize) -> Vec<T> {
    if items.len() > max {
        items.drain(..items.len() - max);
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_cut_text_where_javascript_does() {
        assert_eq!(js_len("año"), 3);
        assert_eq!(js_len("a😀"), 3);
        assert_eq!(js_prefix("año nuevo", 3), "año");
        // An emoji is two units: never split.
        assert_eq!(js_prefix("a😀b", 2), "a");
        assert_eq!(clip("abcdef", 4), "abc…");
        assert_eq!(clip("abc", 4), "abc");
        assert_eq!(flat("  a \n\t b  "), "a b");
        assert_eq!(keep_last(vec![1, 2, 3], 2), vec![2, 3]);
    }
}
