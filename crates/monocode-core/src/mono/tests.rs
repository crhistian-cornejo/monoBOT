use super::*;
use habits::{Habit, Schedule};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn identity_projects_order_and_hidden_sessions_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("native.sqlite")).unwrap();
    let mut roster = Roster::default();
    let a = roster
        .create(vec!["/one/app".into(), "/two/app".into()])
        .id
        .clone();
    let other = roster.create(vec!["/else".into()]).id.clone();
    let b = roster.create(vec!["/one/app".into()]).id.clone();
    assert!(roster.get(&a).unwrap().project(Some("app")).is_err());
    assert!(roster.get(&a).unwrap().project(Some("/outside")).is_err());
    assert!(roster.get(&a).unwrap().project(None).is_err());
    assert_eq!(roster.get(&b).unwrap().project(None).unwrap(), "/one/app");
    roster.reorder(&[b.clone(), a.clone()]);
    roster.hidden_sessions.push(17);
    roster.save(&db).unwrap();
    let saved = Roster::load(&db).unwrap();
    assert_eq!(
        saved.monos.iter().map(|m| &m.id).collect::<Vec<_>>(),
        vec![&b, &other, &a]
    );
    assert_eq!(saved.hidden_sessions, vec![17]);
    assert_ne!(saved.monos[0].mascot, saved.monos[1].mascot);
}
#[test]
fn a_workstream_orchestrator_survives_restart_with_its_tour_seen() {
    let temp = tempfile::tempdir().unwrap();
    let db = Store::open(&temp.path().join("native.sqlite")).unwrap();
    let mut roster = Roster::default();
    roster.create(Vec::new());
    let workstream = workstreams::Workstream {
        project: "/repo".into(),
        cwd: "/repo-ws".into(),
        slug: "migrate".into(),
        folder: "/data/workstreams/ws".into(),
        created_at: 1,
    };
    let mono = roster
        .create_workstream(workstreams::Created {
            id: "ws".into(),
            name: "Migrate".into(),
            workstream: workstream.clone(),
        })
        .clone();
    assert_eq!(mono.kind.as_deref(), Some("workstream"));
    assert_eq!(mono.projects, ["/repo"]);
    assert!(mono.is_workstream() && !mono.is_coding() && !mono.is_personal());
    assert!(mono.works_on("/repo"));
    assert_ne!(mono.mascot, roster.monos[0].mascot);
    roster.workstream_intro_seen = true;
    roster.save(&db).unwrap();
    let saved = Roster::load(&db).unwrap();
    assert!(saved.workstream_intro_seen);
    let back = saved.get("ws").unwrap();
    assert_eq!(back.label(), "Migrate");
    assert_eq!(back.workstream.as_ref(), Some(&workstream));
}
#[test]
fn an_agent_message_names_its_sender_for_route_recovery() {
    let text = agents::agent_message("Principal", "principal", "Check the invoices");
    assert_eq!(agents::message_sender(&text), Some("Principal"));
    assert!(agents::message_sender("hola").is_none());
    assert!(agents::message_sender("<agent_message from=\"\">x</agent_message>").is_none());
    assert!(agents::message_sender("<agent_message from=").is_none());
}

#[test]
fn memory_supersedes_uniquely_redacts_and_archives_before_prompt_budget() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "memory-test";
    let secret = "sk-1234567890abcdefghijklmnop";
    let entry = memory::entry(&format!("Use Rust. token={secret}"), "2026-10-19", None).unwrap();
    assert!(!entry.contains(secret));
    let first = memory::add("# User notes\n", &entry);
    assert_eq!(memory::add(&first, &entry), first);
    let new = memory::entry("Use GPUI", "2026-10-19", None).unwrap();
    let replaced = memory::replace(&first, "Use Rust", Some(&new), "2026-10-19").unwrap();
    assert!(replaced.contains("~~"));
    assert!(memory::replace(&replaced, "not present", None, "2026-10-19").is_err());
    let mut many = "# User notes\nDo not drop this user-written line.\n".to_string();
    for n in 0..205 {
        many.push_str(&memory::entry(&format!("fact number {n}"), "2026-10-18", None).unwrap());
        many.push('\n');
    }
    memory::edit(root, id, None, |_| Ok(many.clone())).unwrap();
    let file = memory::read(root, id, None).unwrap().text.unwrap();
    assert!(file.lines().count() <= 200);
    assert!(file.contains("Do not drop"));
    assert!(file.contains("fact number 204"));
    assert!(files::read(root, id, None, "memory/archive.md")
        .unwrap()
        .text
        .unwrap()
        .contains("fact number 0"));
    // A loose query scores every line sharing a word; the best match wins.
    let hits = memory::search(root, id, "fact number 204", None).unwrap();
    assert!(hits.len() <= 20);
    assert_eq!(hits[0].line, "- 2026-10-18 · fact number 204");
    assert_eq!(hits[0].date.as_deref(), Some("2026-10-18"));
    assert!(memory::topic("../secret").is_err());
}
#[test]
fn habit_updates_retry_conflicts_without_losing_other_edits() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let id = "habit-test";
    let now = 1_792_411_200_000;
    let a = Habit::new(
        "A".into(),
        "Report changes".into(),
        Schedule::default(),
        now,
    )
    .unwrap();
    let b = Habit::new("B".into(), "Check builds".into(), Schedule::default(), now).unwrap();
    let inject = AtomicBool::new(true);
    habits::update(root, id, |all| {
        if inject.swap(false, Ordering::SeqCst) {
            habits::update(root, id, |other| {
                other.push(b.clone());
                Ok(())
            })?;
        }
        all.push(a.clone());
        Ok(())
    })
    .unwrap();
    let all = habits::load(root, id).unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0].id, b.id);
    assert_eq!(all[1].id, a.id);
    let mut h = a;
    h.enabled = false;
    h.next_run_at = now;
    assert!(!h.due(now));
    h.run_requested = true;
    assert!(h.due(now));
    h.running_since = Some(now);
    assert!(!h.due(now));
    assert!(habits::report("`NOTHING_TO_REPORT`").is_none());
    assert!(habits::report("The build failed.\nNOTHING_TO_REPORT")
        .unwrap()
        .contains("build failed"));
}
#[test]
fn schedules_are_strictly_future_and_reject_invalid_fields() {
    let now = 1_792_411_200_000;
    for kind in ["hourly", "daily", "weekdays", "weekly"] {
        let s = Schedule {
            schedule_kind: kind.into(),
            ..Default::default()
        };
        let next = s.next(now).unwrap();
        assert!(next > now);
        assert!(next - now <= 8 * 86400000);
        assert!(s.next(next).unwrap() > next);
    }
    assert!(Schedule {
        time: "25:00".into(),
        ..Default::default()
    }
    .validate()
    .is_err());
    assert!(Schedule {
        minute: 60,
        ..Default::default()
    }
    .validate()
    .is_err());
    let s: Schedule = serde_json::from_str(r#"{"kind":"hourly","minute":15}"#).unwrap();
    assert_eq!(s.minute, 15);
}
#[test]
fn delivery_receipt_is_saved_with_the_user_message() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("native.sqlite");
    let mut db = Store::open(&path).unwrap();
    let session = db.create("codex", "/project").unwrap();
    let row = Row {
        role: "user".into(),
        text: "hello".into(),
        first: true,
        sent_at: None,
    };
    db.write_rows_with_message(session.id, 0, std::slice::from_ref(&row), Some("message-1"))
        .unwrap();
    drop(db);
    let db = Store::open(&path).unwrap();
    assert_eq!(db.load(session.id).unwrap().rows, vec![row]);
    assert_eq!(
        db.setting(&format!("mono-delivered:{}:message-1", session.id))
            .unwrap()
            .as_deref(),
        Some("1")
    );
}

#[test]
fn context_shows_how_to_delegate_without_running_help() {
    let mono = Mono {
        projects: vec!["/repo".into()],
        ..Default::default()
    };
    let files = files::AgentFiles {
        id: mono.id.clone(),
        dir: String::new(),
        soul: Some(DEFAULT_SOUL.into()),
        soul_hash: String::new(),
        memory: String::new(),
        memory_hash: String::new(),
        memory_path: String::new(),
        topics: Vec::new(),
        team: None,
    };
    let executable = std::path::Path::new("/Applications/Monocode.app/monocode");
    let start = r#"/Applications/Monocode.app/monocode app sessions.start --json '{"project":"<assigned path>","title":"<short title>","prompt":"<complete instructions>","notifyOnComplete":true}'"#;
    let rules = instructions(&mono, executable);
    assert!(rules.contains(start));
    assert!(rules.contains("## Be quick"));
    assert!(rules.contains("- /repo"));
    // Without system instructions, the rules ride along in the message.
    let inline = context(&mono, &files, executable, false);
    assert!(inline.contains(&rules));
    assert!(inline.contains("<soul>"));
    // With them, the message carries only what changes between turns.
    let system = context(&mono, &files, executable, true);
    assert!(!system.contains(start));
    assert!(system.contains("<soul>") && system.contains("<memory>"));
    assert!(system_instructions_supported(crate::provider::Kind::Claude));
    assert!(system_instructions_supported(crate::provider::Kind::Codex));
    assert!(!system_instructions_supported(crate::provider::Kind::Devin));
}

#[test]
fn a_resumed_codex_thread_gets_rules_it_has_not_seen_in_the_message() {
    use crate::provider::Kind;
    // A fresh thread takes them as developer instructions.
    assert!(rules_in_system(Kind::Codex, false, None, "new"));
    // Codex keeps a thread's first instructions, so a moved app CLI or
    // rules from before they lived there must come with the message.
    assert!(!rules_in_system(Kind::Codex, true, None, "new"));
    assert!(!rules_in_system(Kind::Codex, true, Some("old"), "new"));
    assert!(rules_in_system(Kind::Codex, true, Some("new"), "new"));
    // Claude's system prompt is sent with every process.
    assert!(rules_in_system(Kind::Claude, true, None, "new"));
    assert!(!rules_in_system(Kind::Devin, false, None, "new"));
}

#[test]
fn subsessions_fill_only_a_start_that_names_no_model() {
    let set = Subsessions {
        harness: Some("claude".into()),
        model: Some("opus".into()),
        effort: Some("high".into()),
    };
    let mut bare = serde_json::json!({"prompt": "x"});
    set.fill(&mut bare);
    assert_eq!(
        bare,
        serde_json::json!({"prompt": "x", "harness": "claude", "model": "opus", "effort": "high"})
    );
    for chosen in ["harness", "model", "modelSettings", "effort"] {
        let mut named = serde_json::json!({"prompt": "x", chosen: "codex"});
        let before = named.clone();
        set.fill(&mut named);
        assert_eq!(named, before, "{chosen} is the agent's own choice");
    }
    let effort_only = Subsessions {
        effort: Some("low".into()),
        ..Default::default()
    };
    let mut input = serde_json::json!({"prompt": "x"});
    effort_only.fill(&mut input);
    assert_eq!(input, serde_json::json!({"prompt": "x", "effort": "low"}));
}

#[test]
fn subsessions_normalize_away_when_everything_is_inherited() {
    assert_eq!(Subsessions::default().normalized(), None);
    let blank = Subsessions {
        harness: Some(" ".into()),
        ..Default::default()
    };
    assert_eq!(blank.normalized(), None);
    // A model without its provider can't be named.
    let orphan = Subsessions {
        model: Some("opus".into()),
        ..Default::default()
    };
    assert_eq!(orphan.normalized(), None);
}

fn group_wearing(mascot: &str, color: u32) -> Mono {
    Mono {
        id: new_id(),
        mascot: mascot.into(),
        color,
        kind: Some("group".into()),
        group: Some(groups::Group::default()),
        ..Default::default()
    }
}

#[test]
fn a_new_group_looks_random_and_unlike_every_other_group() {
    use rand::SeedableRng;
    let mut roster = Roster::default();
    roster.monos.push(group_wearing(MASCOTS[0], COLORS_MONO[0]));
    roster.monos.push(group_wearing(MASCOTS[3], COLORS_MONO[5]));
    let mut seen = HashSet::new();
    for seed in 0..64 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        let look = roster.next_group_look(&mut rng);
        assert!(MASCOTS.contains(&look.0.as_str()));
        assert!(COLORS_MONO.contains(&look.1));
        assert!(
            !roster
                .monos
                .iter()
                .any(|m| m.is_group() && m.mascot == look.0 && m.color == look.1),
            "{look:?} is already a group's look"
        );
        // While some are free, neither half is worn by another Mono.
        assert!(!roster.monos.iter().any(|m| m.mascot == look.0));
        assert!(!roster.monos.iter().any(|m| m.color == look.1));
        seen.insert(look);
    }
    assert!(seen.len() > 8, "the look is chosen at random: {seen:?}");
    let mut a = rand::rngs::StdRng::seed_from_u64(7);
    let mut b = rand::rngs::StdRng::seed_from_u64(7);
    assert_eq!(
        roster.next_group_look(&mut a),
        roster.next_group_look(&mut b),
        "the same seed gives the same look"
    );
}

#[test]
fn groups_created_one_after_another_never_share_a_look() {
    let root = tempfile::tempdir().unwrap();
    let mut roster = Roster::default();
    roster.create(Vec::new());
    let total = MASCOTS.len() * COLORS_MONO.len();
    for ix in 0..total {
        roster.create_group(
            root.path(),
            &format!("Group {ix}"),
            groups::Group::default(),
            None,
        );
    }
    let looks: HashSet<(String, u32)> = roster
        .monos
        .iter()
        .filter(|m| m.is_group())
        .map(|m| (m.mascot.clone(), m.color))
        .collect();
    assert_eq!(
        looks.len(),
        total,
        "every pair is used once before any repeats"
    );
}

#[test]
fn group_looks_prefer_mascots_and_colors_no_mono_wears() {
    use rand::SeedableRng;
    let mut roster = Roster::default();
    // Every mascot but the last and every color but the last are worn.
    for (ix, mascot) in MASCOTS.iter().enumerate().take(MASCOTS.len() - 1) {
        roster.monos.push(Mono {
            id: new_id(),
            mascot: (*mascot).into(),
            color: COLORS_MONO[ix % (COLORS_MONO.len() - 1)],
            ..Default::default()
        });
    }
    let spare = (
        MASCOTS[MASCOTS.len() - 1].to_string(),
        COLORS_MONO[COLORS_MONO.len() - 1],
    );
    for seed in 0..16 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        assert_eq!(roster.next_group_look(&mut rng), spare);
    }
}

#[test]
fn group_looks_repeat_the_least_worn_once_every_pair_is_taken() {
    use rand::SeedableRng;
    let mut roster = Roster::default();
    // Every pair is a group's, and all but one are worn twice.
    let (spare_mascot, spare_color) = (MASCOTS[2], COLORS_MONO[6]);
    for round in 0..2 {
        for mascot in MASCOTS {
            for color in COLORS_MONO {
                if round == 0 || (mascot, color) != (spare_mascot, spare_color) {
                    roster.monos.push(group_wearing(mascot, color));
                }
            }
        }
    }
    for seed in 0..16 {
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed);
        assert_eq!(
            roster.next_group_look(&mut rng),
            (spare_mascot.to_string(), spare_color)
        );
    }
}
