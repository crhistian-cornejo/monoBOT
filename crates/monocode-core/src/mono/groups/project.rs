//! A group as a project (`groupProject.ts`): what its settings dialog edits
//! and the first memories the user gives a new group.
use super::group::{Folder, Group};
use crate::mono::{files, memory};
use regex::Regex;
use std::{path::Path, sync::LazyLock};

/// A group as its settings dialog edits it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroupDraft {
    pub name: String,
    pub goal: String,
    pub members: Vec<String>,
    pub folders: Vec<Folder>,
    pub context: Vec<String>,
}

/// The draft of a group whose display name is `name`.
pub fn group_draft(name: &str, group: &Group) -> GroupDraft {
    GroupDraft {
        name: name.into(),
        goal: group.goal.clone().unwrap_or_default(),
        members: group.members.clone(),
        folders: group.folders.clone(),
        context: group.context.clone(),
    }
}

static BULLET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[-*]\s*").unwrap());

/// What the user wrote for a new group to remember, as memory entries: one
/// dated entry per line, the way memory.add writes them. An entry memory.add
/// would refuse (over 1000 characters) fails the whole seed, as it does in
/// the desktop app.
pub fn memory_seed(memory: &str, text: &str, date: &str) -> Result<String, String> {
    text.split('\n')
        .map(|line| BULLET.replace(line, "").trim().to_string())
        .filter(|fact| !fact.is_empty())
        .try_fold(memory.to_string(), |current, fact| {
            Ok(memory::add(&current, &memory::entry(&fact, date, None)?))
        })
}

/// Writes a new group's first memories into its MEMORY.md under `root`
/// (the agents' folder); a failure leaves it as it was, and the group
/// learns as it goes.
pub fn seed_group_memory(root: &Path, mono_id: &str, text: &str, date: &str) {
    let Ok(current) = files::read(root, mono_id, None, "MEMORY.md") else {
        return;
    };
    let before = current.text.unwrap_or_default();
    if let Ok(next) = memory_seed(&before, text, date) {
        if next != before {
            let _ = files::save(root, mono_id, None, "MEMORY.md", &next, Some(&current.hash));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_turn_what_the_user_wrote_into_dated_entries() {
        let memory = memory_seed(
            "",
            "- The client is Nexa\n\n* Deadline: Nov 20\r\n",
            "2026-11-05",
        )
        .unwrap();
        assert!(memory.contains("The client is Nexa"));
        assert!(memory.contains("Deadline: Nov 20"));
        assert!(memory.contains("2026-11-05"));
        assert!(!memory.contains('\r'));
        // A fact it already remembers is not written twice.
        assert_eq!(
            memory_seed(&memory, "The client is Nexa", "2026-11-06").unwrap(),
            memory
        );
        assert!(memory_seed("", &"x".repeat(1001), "2026-11-05").is_err());
    }

    #[test]
    fn groups_seed_a_new_groups_memory_file() {
        let root = tempfile::tempdir().unwrap();
        seed_group_memory(root.path(), "nexa", "- The client is Nexa", "2026-11-05");
        let file = files::read(root.path(), "nexa", None, "MEMORY.md").unwrap();
        assert!(
            file.text
                .unwrap()
                .contains("- 2026-11-05 · The client is Nexa")
        );
    }

    #[test]
    fn groups_draft_their_settings() {
        let group = Group {
            members: vec!["ivy".into()],
            goal: Some("Informe".into()),
            ..Group::default()
        };
        let draft = group_draft("Nexa", &group);
        assert_eq!(
            (draft.name.as_str(), draft.goal.as_str()),
            ("Nexa", "Informe")
        );
        assert_eq!(draft.members, vec!["ivy"]);
        assert_eq!(group_draft("Empty", &Group::default()).goal, "");
    }
}
