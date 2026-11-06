use super::*;
use crate::{monos::bot_transfer::Meeting, settings::Section, shell::tests::window};
use gpui::TestAppContext;
use monocode_core::{
    connectors::{self, ConnectorSettings, Platform},
    mono::transfer::ImportPlan,
    skills::bundled,
};

fn leak(text: String) -> &'static str {
    Box::leak(text.into_boxed_str())
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} renders"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
}

fn stored(
    window: gpui::WindowHandle<Shell>,
    visual: &mut gpui::VisualTestContext,
    key: String,
) -> Option<String> {
    window
        .update(visual, |shell, _, _| {
            crate::runtime::locked(&shell.store)
                .unwrap()
                .setting(&key)
                .unwrap()
        })
        .unwrap()
}

#[gpui::test]
fn connector_and_skill_pack_switches_are_stored(cx: &mut TestAppContext) {
    cx.update(super::super::bind_keys);
    let (window, _, _) = window(cx);
    window
        .update(cx, |shell, w, cx| shell.open_settings(Section::Mcp, w, cx))
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let listed = connectors::available_connectors(Platform::current());
    assert!(!listed.is_empty());
    for connector in &listed {
        assert!(
            visual
                .debug_bounds(leak(format!("settings-connector-{}", connector.id)))
                .is_some(),
            "{} is listed",
            connector.id
        );
    }
    let connector = listed[0];
    let was = window
        .update(visual, |shell, _, _| {
            let db = crate::runtime::locked(&shell.store).unwrap();
            ConnectorSettings::from_store(&db).is_enabled(connector)
        })
        .unwrap();
    let key = connectors::enabled_key(connector.id);
    let switch = leak(format!("settings-toggle-{}", connector.id));
    click(visual, switch);
    let saved = stored(window, visual, key.clone());
    assert_eq!(saved.as_deref(), Some(if was { "false" } else { "true" }));
    click(visual, switch);
    let saved = stored(window, visual, key);
    assert_eq!(saved.as_deref(), Some(if was { "true" } else { "false" }));

    window
        .update(visual, |shell, _, cx| {
            shell.select_settings(Section::Skills, cx)
        })
        .unwrap();
    visual.run_until_parked();
    for pack in bundled::SKILL_PACKS.iter() {
        assert!(
            visual
                .debug_bounds(leak(format!("settings-skill-pack-{}", pack.id)))
                .is_some(),
            "{} is listed",
            pack.id
        );
    }
    let pack = bundled::SKILL_PACKS[0].id;
    let switch = leak(format!("settings-toggle-{pack}"));
    click(visual, switch);
    let saved = stored(window, visual, bundled::skill_pack_key(pack));
    assert_eq!(saved.as_deref(), Some("true"));
    click(visual, switch);
    let saved = stored(window, visual, bundled::skill_pack_key(pack));
    assert_eq!(saved.as_deref(), Some("false"));
}

#[gpui::test]
fn bots_on_another_computer_choose_and_turn_off_the_sync_folder(cx: &mut TestAppContext) {
    cx.update(super::super::bind_keys);
    let (window, _, _) = window(cx);
    window
        .update(cx, |shell, w, cx| {
            shell.open_settings(Section::Monos, w, cx)
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    for selector in [
        "settings-row-bots-transfer",
        "settings-row-bots-sync",
        "settings-bots-export",
        "settings-bots-import",
        "settings-bots-sync-choose",
    ] {
        assert!(visual.debug_bounds(selector).is_some(), "{selector}");
    }
    assert!(visual.debug_bounds("settings-bots-sync-now").is_none());
    let folder = std::env::temp_dir().join(format!("monobot-sync-{}", std::process::id()));
    let shared = folder.to_string_lossy().into_owned();
    window
        .update(visual, |shell, _, cx| {
            assert!(shell.monos.is_some());
            shell.bot_sync_folder(Some(shared.clone()), cx)
        })
        .unwrap();
    visual.run_until_parked();
    let saved = stored(window, visual, sync::FOLDER_KEY.into());
    assert_eq!(saved.as_deref(), Some(shared.as_str()));
    for selector in [
        "settings-bots-sync-now",
        "settings-bots-sync-off",
        "settings-bots-sync-folder",
    ] {
        assert!(visual.debug_bounds(selector).is_some(), "{selector}");
    }
    // Sync now runs again, and the run ends with a status line.
    click(visual, "settings-bots-sync-now");
    assert!(visual.debug_bounds("settings-bots-sync-status").is_some());
    click(visual, "settings-bots-sync-off");
    let saved = stored(window, visual, sync::FOLDER_KEY.into());
    assert_eq!(saved.as_deref(), Some(""));
    assert!(visual.debug_bounds("settings-bots-sync-now").is_none());
    assert!(visual.debug_bounds("settings-bots-sync-choose").is_some());
}

fn bot(name: &str) -> Bot {
    Bot {
        name: name.into(),
        mascot: "ghost".into(),
        color: "#7c3aed".into(),
        soul: format!("# {name}\nHelp the user.\n"),
        ..Default::default()
    }
}

#[gpui::test]
fn importing_adds_new_bots_and_asks_about_the_ones_that_exist(cx: &mut TestAppContext) {
    cx.update(super::super::bind_keys);
    let (window, _, _) = window(cx);
    window
        .update(cx, |shell, w, cx| {
            shell.open_settings(Section::Monos, w, cx)
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let current = bot("Research");
    let incoming = Bot {
        memory: "- Prefers Spanish\n".into(),
        ..bot("Research")
    };
    let comparison = transfer::compare(&current, &incoming);
    let prepared = PreparedImport {
        plan: ImportPlan {
            fresh: vec![bot("Imported Test Bot")],
            conflicts: vec![transfer::Conflict {
                incoming,
                existing_id: "missing".into(),
                choice: transfer::Choice::Keep,
            }],
        },
        meetings: vec![Some(Meeting {
            current,
            comparison,
        })],
    };
    window
        .update(visual, |shell, w, cx| {
            shell.open_bot_import("windows", "", prepared, None, w, cx)
        })
        .unwrap();
    visual.run_until_parked();
    for selector in [
        "bot-import-keep",
        "bot-import-replace",
        "bot-import-both",
        "bot-import-apply",
    ] {
        assert!(visual.debug_bounds(selector).is_some(), "{selector}");
    }
    let dialog = |window: gpui::WindowHandle<Shell>, visual: &mut gpui::VisualTestContext| {
        window
            .update(visual, |shell, _, cx| {
                shell.bot_import.as_ref().unwrap().view.read(cx).choices()
            })
            .unwrap()
    };
    // The choices end a long body: scroll them into view like a user would.
    let body = visual.debug_bounds("bot-import-body").unwrap();
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: body.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-3000.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    // The suggestion (the incoming one has more memory) is the default.
    assert_eq!(dialog(window, visual), vec![transfer::Choice::Replace]);
    click(visual, "bot-import-keep");
    assert_eq!(dialog(window, visual), vec![transfer::Choice::Keep]);
    click(visual, "bot-import-both");
    assert_eq!(dialog(window, visual), vec![transfer::Choice::Both]);
    click(visual, "bot-import-apply");
    // The new bot is added and "Keep both" copies the incoming one.
    window
        .update(visual, |shell, _, cx| {
            if let Some(open) = &shell.bot_import {
                panic!("import failed: {:?}", open.view.read(cx).error());
            }
            let monos = shell.monos.as_ref().unwrap().read(cx);
            let names: Vec<String> = monos.roster.monos.iter().map(|m| m.label()).collect();
            assert!(names.iter().any(|n| n == "Imported Test Bot"), "{names:?}");
            assert!(names.iter().any(|n| n.starts_with("Research")), "{names:?}");
            let status = shell.settings.as_ref().unwrap().read(cx).bots_status();
            assert_eq!(
                status,
                Some(&(
                    "Bots imported: 1 added, 1 kept as a copy.".to_owned(),
                    false
                ))
            );
        })
        .unwrap();
}
