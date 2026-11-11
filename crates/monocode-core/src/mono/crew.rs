//! Chief's crew: the user's persistent code monos (bots with the `coder`
//! role) and Chief itself (the `chief` role). A Chief run gives each task to
//! a free mono whose name or specialty fits, and makes a new one only when
//! none is free; every mono stays with the user afterwards.
use super::{files, Mono, Roster};
use crate::orchestration::control::CrewMember;
use std::path::Path;

pub const CHIEF: &str = "chief";
pub const CODER: &str = "coder";

fn has_role(mono: &Mono, role: &str) -> bool {
    mono.kind.as_deref() == Some("bot") && mono.bot_role.as_deref() == Some(role)
}

pub fn is_coder(mono: &Mono) -> bool {
    has_role(mono, CODER)
}

pub fn is_chief(mono: &Mono) -> bool {
    has_role(mono, CHIEF)
}

/// The crew Chief sees: every code mono, by name and specialty.
pub fn members(roster: &Roster) -> Vec<CrewMember> {
    roster
        .monos
        .iter()
        .filter(|m| is_coder(m))
        .map(|m| CrewMember {
            id: m.id.clone(),
            name: m.name.clone(),
            specialty: m.specialty.clone().unwrap_or_default(),
        })
        .collect()
}

/// Chief's own mono, made the first time a chat hands work to Chief.
/// Returns its id and whether it was just made.
pub fn ensure_chief(roster: &mut Roster, root: &Path) -> Option<(String, bool)> {
    if let Some(chief) = roster.monos.iter().find(|m| is_chief(m)) {
        return Some((chief.id.clone(), false));
    }
    roster
        .create_bot(root, CHIEF, false)
        .map(|m| (m.id.clone(), true))
}

fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect()
}

/// A free code mono for a task: one whose name is the task's handle, else
/// the one whose specialty shares the most words with the handle and the
/// title, else any free one. None when every code mono is busy.
pub fn pick(
    roster: &Roster,
    handle: Option<&str>,
    title: &str,
    busy: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let free: Vec<&Mono> = roster
        .monos
        .iter()
        .filter(|m| is_coder(m) && !busy(&m.id))
        .collect();
    if let Some(handle) = handle {
        if let Some(named) = free.iter().find(|m| m.name.eq_ignore_ascii_case(handle)) {
            return Some(named.id.clone());
        }
    }
    let wanted = words(&format!("{} {title}", handle.unwrap_or_default()));
    free.iter()
        .enumerate()
        .max_by_key(|(ix, m)| {
            let known = words(&format!(
                "{} {}",
                m.name,
                m.specialty.as_deref().unwrap_or_default()
            ));
            let shared = wanted.iter().filter(|w| known.contains(w)).count();
            // Ties go to the oldest mono.
            (shared, std::cmp::Reverse(*ix))
        })
        .map(|(_, m)| m.id.clone())
}

/// The name a new code mono gets: its handle, capitalised, numbered when
/// another mono already has it.
fn name_for(roster: &Roster, handle: Option<&str>) -> String {
    let base = handle
        .map(|h| h.replace(['-', '_'], " "))
        .map(|h| {
            let mut chars = h.trim().chars();
            chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect::<String>())
                .unwrap_or_default()
        })
        .filter(|h| !h.is_empty() && !h.to_lowercase().starts_with("worker"))
        .unwrap_or_else(|| "Coder".into());
    let taken = |name: &str| {
        roster
            .monos
            .iter()
            .any(|m| m.name.eq_ignore_ascii_case(name))
    };
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .unwrap()
}

/// A new code mono for a task: named for its handle, with a specialty that
/// says what it was made for. It stays as one of the user's agents.
pub fn create(
    roster: &mut Roster,
    root: &Path,
    handle: Option<&str>,
    title: &str,
) -> Option<String> {
    let name = name_for(roster, handle);
    let mono = roster.create_bot(root, CODER, false)?;
    let id = mono.id.clone();
    let mono = roster.monos.iter_mut().find(|m| m.id == id)?;
    mono.name = name;
    mono.specialty = Some(match handle {
        Some(handle) => format!("Code mono ({handle}): {title}"),
        None => format!("Code mono: {title}"),
    });
    Some(id)
}

/// The block a worker's first turn starts with: who it is, its soul and
/// memory, without the chat rules a bot's own chat has.
pub fn identity(root: &Path, mono: &Mono) -> String {
    let loaded = files::load(root, &mono.id, None).ok();
    let soul = loaded
        .as_ref()
        .and_then(|f| f.soul.clone())
        .unwrap_or_default();
    let memory = loaded.map(|f| f.memory).unwrap_or_default();
    let mut block = format!(
        "<monocode_mono>\nYou are {}, one of the user's code monos{}. Chief gave you the task below.",
        mono.name,
        mono.specialty
            .as_deref()
            .map(|s| format!(" ({s})"))
            .unwrap_or_default()
    );
    if !soul.trim().is_empty() {
        block.push_str(&format!("\n<soul>\n{}\n</soul>", soul.trim()));
    }
    if !memory.trim().is_empty() {
        block.push_str(&format!("\n<memory>\n{}\n</memory>", memory.trim()));
    }
    block.push_str("\n</monocode_mono>");
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roster() -> (Roster, tempfile::TempDir) {
        (Roster::default(), tempfile::tempdir().unwrap())
    }

    #[test]
    fn chief_is_made_once_and_kept() {
        let (mut roster, root) = roster();
        let (id, made) = ensure_chief(&mut roster, root.path()).unwrap();
        assert!(made);
        assert_eq!(
            ensure_chief(&mut roster, root.path()).unwrap(),
            (id.clone(), false)
        );
        let chief = roster.monos.iter().find(|m| m.id == id).unwrap();
        assert!(is_chief(chief));
        assert_eq!(chief.name, "Chief");
        assert!(members(&roster).is_empty(), "Chief is not one of its crew");
    }

    #[test]
    fn a_task_goes_to_a_free_mono_that_fits_before_a_new_one_is_made() {
        let (mut roster, root) = roster();
        let none = |_: &str| false;
        assert_eq!(pick(&roster, Some("tester"), "Add tests", &none), None);
        let tester = create(&mut roster, root.path(), Some("tester"), "Add auth tests").unwrap();
        let docs = create(&mut roster, root.path(), Some("writer"), "Write the docs").unwrap();
        let names: Vec<_> = members(&roster).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["Tester", "Writer"]);
        assert_eq!(
            pick(&roster, Some("tester"), "More tests", &none).as_deref(),
            Some(tester.as_str())
        );
        // No name fits: the specialty does.
        assert_eq!(
            pick(&roster, Some("reviewer"), "Update the docs", &none).as_deref(),
            Some(docs.as_str())
        );
        // A busy mono is skipped; with every mono busy, none is picked.
        let tester_busy = |id: &str| id == tester;
        assert_eq!(
            pick(&roster, Some("tester"), "Tests", &tester_busy).as_deref(),
            Some(docs.as_str())
        );
        assert_eq!(pick(&roster, None, "x", &|_| true), None);
        // The same handle again gets a numbered name.
        create(&mut roster, root.path(), Some("tester"), "Fuzz").unwrap();
        assert!(roster.monos.iter().any(|m| m.name == "Tester 2"));
        // A generic worker handle is a plain Coder.
        create(&mut roster, root.path(), Some("worker-3"), "Fix").unwrap();
        assert!(roster.monos.iter().any(|m| m.name == "Coder"));
    }

    #[test]
    fn a_worker_starts_as_its_mono() {
        let (mut roster, root) = roster();
        let id = create(&mut roster, root.path(), Some("tester"), "Add tests").unwrap();
        files::save(
            root.path(),
            &id,
            None,
            "MEMORY.md",
            "The suite runs with cargo xtask test.",
            None,
        )
        .unwrap();
        let mono = roster.monos.iter().find(|m| m.id == id).unwrap();
        let block = identity(root.path(), mono);
        assert!(block.starts_with("<monocode_mono>\nYou are Tester"));
        assert!(block.contains("<soul>"));
        assert!(block.contains("cargo xtask test"));
    }
}
