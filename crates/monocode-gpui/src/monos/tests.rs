use super::*;
use gpui::{TestAppContext, WindowHandle};
use monocode_core::store::Store;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
struct Setup {
    window: WindowHandle<Shell>,
    manager: Entity<Monos>,
    id: String,
    project: String,
    session: i64,
    root: PathBuf,
    store: Database,
}

#[gpui::test]
fn missing_document_uses_error_cards_without_moving_content(cx: &mut TestAppContext) {
    let s = setup(cx);
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    let body = visual.debug_bounds("main-pane-frame").unwrap();
    s.manager.update(visual, |m, cx| {
        m.failed_reports.insert("failed-report".into());
        m.open_document("deleted-document", cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert_eq!(
        s.manager
            .read_with(visual, |m, _| m.error.clone())
            .as_deref(),
        Some("Document unavailable. It may have been deleted.")
    );
    let error = visual.debug_bounds("mono-error").unwrap();
    let viewport = s
        .window
        .update(visual, |_, w, _| w.viewport_size())
        .unwrap();
    assert_eq!(error.top(), px(12.));
    assert_eq!(error.right(), viewport.width - px(12.));
    assert_eq!(error.size.width, px(360.));
    assert_eq!(visual.debug_bounds("main-pane-frame").unwrap(), body);

    let close = visual.debug_bounds("mono-error-close").unwrap();
    visual.simulate_click(close.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("mono-error").is_none());
    s.manager.read_with(visual, |m, _| {
        assert!(m.error.is_none());
        assert!(m.failed_reports.is_empty());
        assert!(
            m.settings.is_some(),
            "dismissing an error keeps settings open"
        );
        assert_eq!(m.active.as_deref(), Some(s.id.as_str()));
    });
}

#[gpui::test]
fn saved_desktop_mono_opens_in_actual_manager_without_replacing_its_conversation(
    cx: &mut TestAppContext,
) {
    let db = Store::open_monocode(std::path::Path::new(":memory:")).unwrap();
    let session = db.create("codex", "/saved-project").unwrap();
    let original = db.persisted_session_id(session.id).unwrap();
    let settings = Settings {
        model: Some("gpt-6.1-sol".into()),
        access: AccessMode::Auto,
        ..Default::default()
    };
    db.configure_agent(session.id, "codex", &settings).unwrap();
    let mut roster = Roster::default();
    roster.monos.push(Mono {
        id: "desktop-original-mono".into(),
        session_id: Some(session.id),
        name: "Big Boss".into(),
        mascot: "ghost".into(),
        color: 0xe25a9e,
        projects: vec!["/saved-project".into()],
        ..Default::default()
    });
    roster.save(&db).unwrap();
    let summaries = db.sessions().unwrap();
    let store = Arc::new(Mutex::new(db));
    let database = store.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(database, summaries, "/saved-project".into(), None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let manager = window
        .update(cx, |shell, _, _| shell.mono_manager())
        .unwrap();
    window
        .update(cx, |_, window, cx| {
            manager.update(cx, |m, cx| m.open("desktop-original-mono", window, cx));
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        manager.update(cx, |m, cx| {
            m.ensure_chat("desktop-original-mono", cx);
            let mono = m.current().unwrap();
            assert_eq!(mono.id, "desktop-original-mono");
            assert_eq!(mono.session_id, Some(session.id));
            assert_eq!(mono.name, "Big Boss");
            assert_eq!(mono.mascot, "ghost");
            let chat = m.chats[&mono.id].read(cx);
            assert_eq!(chat.summary.id, session.id);
            assert_eq!(chat.summary.provider, "codex");
            assert_eq!(chat.settings.model.as_deref(), Some("gpt-6.1-sol"));
            assert_eq!(m.chats.len(), 1);
        });
    });
    let db = locked(&store).unwrap();
    assert_eq!(db.sessions().unwrap().len(), 1);
    assert_eq!(db.persisted_session_id(session.id).unwrap(), original);
}

#[gpui::test]
fn settings_reset_restores_only_mono_soul_and_name(cx: &mut TestAppContext) {
    let s = setup(cx);
    mono::files::save(&s.root, &s.id, None, "SOUL.md", "# Custom soul", None).unwrap();
    mono::files::save(&s.root, &s.id, None, "MEMORY.md", "Keep this memory", None).unwrap();
    let before = cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.roster.get_mut(&s.id).unwrap().name = "Custom name".into();
            m.save(cx);
            m.roster.get(&s.id).unwrap().clone()
        })
    });
    cx.run_until_parked();
    let _reset = cx.update(|cx| s.manager.update(cx, |m, cx| m.reset_defaults(&s.id, cx)));
    cx.run_until_parked();
    let roster = Roster::load(&locked(&s.store).unwrap()).unwrap();
    let after = roster.get(&s.id).unwrap();
    assert!(after.name.is_empty());
    assert_eq!(after.session_id, before.session_id);
    assert_eq!(after.projects, before.projects);
    assert_eq!(after.habit_sessions, before.habit_sessions);
    let files = mono::files::load(&s.root, &s.id, None).unwrap();
    assert_eq!(files.soul.as_deref(), Some(mono::DEFAULT_SOUL));
    assert_eq!(files.memory, "Keep this memory");
}

#[gpui::test]
fn detail_pages_slide_over_the_overview_and_back_without_recreating_content(
    cx: &mut TestAppContext,
) {
    for kind in ["markdown", "html", "session"] {
        cx.update(|cx| cx.set_reduce_motion(true));
        let s = setup(cx);
        let session = s
            .window
            .update(cx, |shell, _, _| shell.workspace.active.unwrap())
            .unwrap();
        s.window
            .update(cx, |_, _, cx| {
                s.manager.update(cx, |m, cx| {
                    m.settings = None;
                    m.overview_tab = OverviewTab::Documents;
                    m.roster
                        .get_mut(&s.id)
                        .unwrap()
                        .documents
                        .push(mono::Document {
                            id: "slide-document".into(),
                            kind: kind.into(),
                            title: "Slide document".into(),
                            summary: String::new(),
                            body: "# Document\n\nKeep this reader alive.".into(),
                            created_at: 1,
                            updated_at: 1,
                        });
                    cx.notify();
                });
            })
            .unwrap();
        let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
        visual.run_until_parked();
        visual.update(|_, cx| cx.set_reduce_motion(false));
        let action = if kind == "session" {
            json!({"type":"session", "sessionId":session})
        } else {
            json!({"type":"document", "documentId":"slide-document"})
        };
        s.window
            .update(visual, |_, _, cx| {
                s.manager
                    .update(cx, |m, cx| m.card_action(&s.id, &action, cx))
            })
            .unwrap();
        visual.run_until_parked();
        let page: &'static str = Box::leak(
            format!(
                "mono-detail-page-{}",
                if kind == "session" {
                    format!("session:{session}")
                } else {
                    "document:slide-document".into()
                }
            )
            .into_boxed_str(),
        );
        let advance = |visual: &mut gpui::VisualTestContext, ms| {
            let details = s.manager.read_with(visual, |m, _| m.details.clone());
            details.update(visual, |d, cx| d.elapsed(Duration::from_millis(ms), cx));
            visual
                .update_window(s.window.into(), |_, w, cx| {
                    w.simulate_next_frame(cx);
                    w.draw(cx).clear(cx);
                })
                .unwrap();
        };
        advance(visual, 70);
        let stack = visual.debug_bounds("mono-detail-stack").unwrap();
        let incoming = visual.debug_bounds(page).unwrap();
        assert!(incoming.left() > stack.left() && incoming.left() < stack.right());
        assert_eq!(
            incoming.size, stack.size,
            "sliding pages keep their full layout size"
        );
        assert!(visual.debug_bounds("mono-detail-base").unwrap().left() < stack.left());
        advance(visual, 1000);
        assert_eq!(visual.debug_bounds(page).unwrap().left(), stack.left());
        let reader = s.manager.read_with(visual, |m, cx| match kind {
            "html" => m.html_documents["slide-document"].entity_id(),
            "session" => m
                .owner
                .upgrade()
                .unwrap()
                .read(cx)
                .mono_session_entity(session)
                .unwrap()
                .entity_id(),
            _ => m.documents["slide-document"].entity_id(),
        });
        assert_eq!(
            s.manager
                .read_with(visual, |m, cx| m.native_detail_content(cx)),
            kind == "html"
        );
        let back = visual.debug_bounds("mono-page-back").unwrap().center();
        visual.simulate_click(back, Default::default());
        visual.run_until_parked();
        advance(visual, 70);
        let outgoing = visual
            .debug_bounds(page)
            .expect("Back keeps the outgoing page mounted");
        assert!(outgoing.left() > stack.left() && outgoing.left() < stack.right());
        advance(visual, 1000);
        assert!(visual.debug_bounds(page).is_none());
        assert_eq!(
            visual.debug_bounds("mono-detail-base").unwrap().left(),
            stack.left()
        );
        assert!(
            !s.manager
                .read_with(visual, |m, cx| m.native_detail_content(cx))
        );

        visual.update(|_, cx| cx.set_reduce_motion(true));
        s.window
            .update(visual, |_, _, cx| {
                s.manager
                    .update(cx, |m, cx| m.card_action(&s.id, &action, cx))
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(visual.debug_bounds(page).unwrap().left(), stack.left());
        s.manager.read_with(visual, |m, cx| {
            let reopened = match kind {
                "html" => m.html_documents["slide-document"].entity_id(),
                "session" => m
                    .owner
                    .upgrade()
                    .unwrap()
                    .read(cx)
                    .mono_session_entity(session)
                    .unwrap()
                    .entity_id(),
                _ => m.documents["slide-document"].entity_id(),
            };
            assert_eq!(reader, reopened);
        });
        visual.update(|_, cx| cx.set_reduce_motion(false));
        s.window
            .update(visual, |_, w, cx| {
                s.manager.update(cx, |m, cx| {
                    m.close(cx);
                    m.open(&s.id, w, cx);
                });
            })
            .unwrap();
        visual.run_until_parked();
        assert!(
            visual.debug_bounds(page).is_none(),
            "returning to a Mono must not replay an old page's exit"
        );
        assert_eq!(
            visual.debug_bounds("mono-detail-base").unwrap().left(),
            stack.left()
        );
    }
}
fn setup(cx: &mut TestAppContext) -> Setup {
    let dir = std::env::temp_dir().join(format!("native-monos-{}", mono::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let project = dir.to_string_lossy().to_string();
    let store = Arc::new(Mutex::new(Store::open(&dir.join("native.sqlite")).unwrap()));
    let db = store.clone();
    let cwd = project.clone();
    let window = cx.update(|cx| {
        cx.open_window(Default::default(), |_, cx| {
            cx.new(|cx| Shell::new(db, Vec::new(), cwd, None, cx))
        })
        .unwrap()
    });
    cx.run_until_parked();
    let manager = window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_open = true;
            cx.notify();
            shell.mono_manager()
        })
        .unwrap();
    let handle: gpui::AnyWindowHandle = window.into();
    handle
        .update(cx, |_, w, cx| {
            manager.update(cx, |m, cx| m.create(vec![project.clone()], w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    let (id, session, root) = cx.update(|cx| {
        let m = manager.read(cx);
        let mono = m.current().unwrap();
        (mono.id.clone(), mono.session_id.unwrap(), m.root.clone())
    });
    Setup {
        window,
        manager,
        id,
        project,
        session,
        root,
        store,
    }
}

#[gpui::test]
fn heartbeat_runs_keep_the_monos_selected_permissions(cx: &mut TestAppContext) {
    for mode in [
        AccessMode::ReadOnly,
        AccessMode::Supervised,
        AccessMode::AutoAcceptEdits,
        AccessMode::Auto,
        AccessMode::FullAccess,
    ] {
        let s = setup(cx);
        s.manager.update(cx, |m, cx| {
            m.chats[&s.id].update(cx, |chat, _| {
                // Avoid catalog probes or provider requests: exercise the actual
                // background runner, stopping before it dispatches a prompt.
                chat.settings.access = mode;
                chat.settings.usage_limit = Some(Default::default());
            });
            m.start_heartbeat(&s.id, "Check for follow-ups".into(), cx);
        });
        cx.run_until_parked();
        s.manager.read_with(cx, |m, cx| {
            assert!(m.error.is_none(), "heartbeat failed: {:?}", m.error);
            let run = &m.running[&m.active_run.expect("heartbeat session was created")];
            assert_eq!(run.view.read(cx).settings.access, mode);
            assert!(
                !run.view.read(cx).busy,
                "the fixture must not start a provider"
            );
        });
    }
}

fn call(
    s: &Setup,
    request: &str,
    action: &str,
    input: Value,
    cx: &mut TestAppContext,
) -> mpsc::Receiver<Result<Value, String>> {
    let (reply, rx) = mpsc::channel();
    let call = mono::bridge::Call {
        session: s.session,
        request_id: request.into(),
        action: action.into(),
        input,
        reply,
    };
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.app_call(call, w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    rx
}
#[gpui::test]
fn settings_match_react_dialog_and_left_navigation_geometry(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let dialog = visual.debug_bounds("mono-settings-dialog").unwrap();
    let nav = visual.debug_bounds("mono-settings-navigation").unwrap();
    let viewport = s
        .window
        .update(visual, |_, w, _| w.viewport_size())
        .unwrap();
    assert_eq!(
        dialog.size.width + px(2.),
        px(880.).min(viewport.width - px(24.))
    );
    assert!((dialog.center().x - viewport.width / 2.).abs() <= px(1.));
    assert_eq!(nav.size.width, px(192.));
    for section in ["General", "Soul", "Memory"] {
        let row = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-{section}").into_boxed_str(),
            ))
            .unwrap();
        let icon = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-icon-{section}").into_boxed_str(),
            ))
            .unwrap();
        let label = visual
            .debug_bounds(Box::leak(
                format!("mono-settings-label-{section}").into_boxed_str(),
            ))
            .unwrap();
        assert_eq!(row.size.height, px(32.));
        assert_eq!(icon.left() - row.left(), px(10.));
        assert_eq!(label.left() - icon.right(), px(10.));
    }
    assert_eq!(
        visual.debug_bounds("mono-look").unwrap().size,
        gpui::size(px(80.), px(80.))
    );
    assert_eq!(
        visual.debug_bounds("mono-name-field").unwrap().size,
        gpui::size(px(256.), px(32.))
    );
}

#[gpui::test]
fn settings_color_popover_follows_its_mascot_when_the_window_resizes(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        crate::shell::bind_keys(cx);
        crate::popover::bind(cx);
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let face = visual.debug_bounds("mono-look").unwrap();
    visual.simulate_click(face.center(), Default::default());
    visual.run_until_parked();
    for size in [
        gpui::size(px(1300.), px(900.)),
        gpui::size(px(1050.), px(760.)),
    ] {
        visual.simulate_resize(size);
        visual.run_until_parked();
        let face = visual.debug_bounds("mono-look").unwrap();
        let picker = visual.debug_bounds("mono-settings-look-picker").unwrap();
        assert!((picker.center().x - face.center().x).abs() <= px(1.));
        assert!(picker.top() >= face.bottom());
    }
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|_, cx| {
        let m = s.manager.read(cx);
        assert!(m.picker.is_none());
        assert!(m.settings.is_some());
    });
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.update(|_, cx| assert!(s.manager.read(cx).settings.is_none()));
}

#[gpui::test]
fn memory_editor_saves_with_enter_and_stays_open_for_the_next_fact(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(input::bind);
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = Some(SettingsSection::Memory);
                m.memory_adding = true;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.window
        .update(visual, |_, w, cx| {
            let field = s.manager.read(cx).fields[&format!("memory-fact:{}", s.id)].clone();
            w.focus(&field.focus_handle(cx), cx);
        })
        .unwrap();
    visual.simulate_input("Keep UI aligned with React.");
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert!(
        mono::files::read(&s.root, &s.id, None, "MEMORY.md")
            .unwrap()
            .text
            .unwrap()
            .contains("Keep UI aligned with React.")
    );
    visual.update(|_, cx| {
        let m = s.manager.read(cx);
        assert!(m.memory_adding);
        assert!(
            m.fields[&format!("memory-fact:{}", s.id)]
                .read(cx)
                .content
                .is_empty()
        );
    });
}

#[gpui::test]
fn group_settings_show_every_members_avatar_and_wrap(cx: &mut TestAppContext) {
    let s = setup(cx);
    let members = s.manager.update(cx, |m, cx| {
        let root = m.root.clone();
        let mut members = vec![
            m.roster
                .create_bot(&root, "principal", false)
                .unwrap()
                .id
                .clone(),
        ];
        for _ in 0..6 {
            members.push(
                m.roster
                    .create_bot(&root, "assistant", false)
                    .unwrap()
                    .id
                    .clone(),
            );
        }
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.kind = Some("group".into());
        mono.group = Some(monocode_core::mono::groups::Group {
            members: members.clone(),
            ..Default::default()
        });
        cx.notify();
        members
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    for size in [
        gpui::size(px(1200.), px(900.)),
        gpui::size(px(700.), px(900.)),
    ] {
        visual.simulate_resize(size);
        visual.run_until_parked();
        visual.refresh().unwrap();
        let container = visual.debug_bounds("mono-settings-members").unwrap();
        for id in &members {
            let selector = Box::leak(format!("mono-settings-member-{id}").into_boxed_str());
            let bounds = visual
                .debug_bounds(selector)
                .expect("every member has an avatar");
            assert!(bounds.left() >= container.left());
            assert!(bounds.right() <= container.right());
            assert!(bounds.bottom() <= container.bottom());
        }
        assert!(
            visual.debug_bounds("mono-look").is_none(),
            "groups do not use a single unrelated mascot"
        );
    }
}

#[gpui::test]
fn memory_learning_refreshes_and_persists_for_monos_bots_and_groups(cx: &mut TestAppContext) {
    use mono::groups::{learning, native};
    for kind in ["mono", "assistant", "principal", "group"] {
        let s = setup(cx);
        let (chat_id, last) = s.manager.update(cx, |m, cx| {
            if kind != "mono" {
                let mono = m.roster.get_mut(&s.id).unwrap();
                mono.kind = Some(if kind == "group" { "group" } else { "bot" }.into());
                mono.bot_role = (kind != "group").then(|| kind.into());
                mono.group = (kind == "group").then(Default::default);
            }
            m.settings = Some(SettingsSection::Memory);
            m.chats[&s.id].update(cx, |chat, cx| {
                let mut transcript = monocode_core::transcript::Transcript::default();
                transcript.begin("user");
                transcript.append("Usaremos el informe de octubre");
                transcript.begin("assistant");
                transcript.append("De acuerdo");
                chat.mono_append(transcript.rows, cx);
            });
            let mono = m.roster.get(&s.id).unwrap().clone();
            m.chats[&s.id].update(cx, |chat, _| chat.busy = true);
            assert!(m.pending_learning(&mono, &HashMap::new(), cx).is_none());
            m.chats[&s.id].update(cx, |chat, _| chat.busy = false);
            let (chat_id, exchange) = m.pending_learning(&mono, &HashMap::new(), cx).unwrap();
            m.finish_learning(
                &s.id,
                &chat_id,
                &exchange.last_id,
                Err("Temporary failure".into()),
                cx,
            );
            assert!(m.pending_learning(&mono, &HashMap::new(), cx).is_none());
            m.groups.learn_failed.insert(
                format!("{chat_id}:{}", exchange.last_id),
                monocode_core::time::now_ms() - 60_001,
            );
            assert!(
                m.pending_learning(&mono, &HashMap::new(), cx).is_some(),
                "transient failures are retried"
            );
            (chat_id, exchange.last_id)
        });
        learning::save_learned(&s.root, &s.id, "- Usaremos el informe de octubre").unwrap();
        mono::memory::edit(&s.root, mono::memory::TEAM_MEMORY_ID, None, |_| {
            Ok("- 2026-11-07 · Shared preference: Spanish reports\n".into())
        })
        .unwrap();
        s.manager.update(cx, |m, cx| {
            m.finish_learning(&s.id, &chat_id, &last, Ok(()), cx)
        });
        cx.run_until_parked();
        s.manager.update(cx, |m, cx| {
            assert!(
                m.files
                    .as_ref()
                    .unwrap()
                    .memory
                    .contains("informe de octubre")
            );
            let mono = m.roster.get(&s.id).unwrap().clone();
            assert!(
                m.pending_learning(&mono, &HashMap::new(), cx).is_none(),
                "a learned exchange is not repeated"
            );
            let files = mono::load_files(&s.root, &mono).unwrap();
            assert!(
                mono::context(&mono, &files, std::path::Path::new("/app"), false)
                    .contains("Shared preference: Spanish reports")
            );
            assert!(
                mono::context(&mono, &files, std::path::Path::new("/app"), false)
                    .contains("informe de octubre")
            );
        });
        assert_eq!(
            native::load_learned(&locked(&s.store).unwrap()).learned_up_to(&chat_id),
            Some(last.as_str())
        );
    }
}

#[gpui::test]
fn memory_actions_update_private_shared_and_topic_memory(cx: &mut TestAppContext) {
    let s = setup(cx);
    for (suffix, extra) in [
        ("private", json!({})),
        ("shared", json!({"shared":true})),
        ("topic", json!({"topic":"reports"})),
    ] {
        let mut input = extra.clone();
        input["fact"] = json!("Use the October report");
        assert!(
            call(&s, &format!("add-{suffix}"), "memory.add", input, cx)
                .recv()
                .unwrap()
                .is_ok()
        );
        let read = call(
            &s,
            &format!("read-{suffix}"),
            "memory.read",
            extra.clone(),
            cx,
        )
        .recv()
        .unwrap()
        .unwrap();
        assert!(read["text"].as_str().unwrap().contains("October report"));
        let mut input = extra.clone();
        input["find"] = json!("October report");
        input["fact"] = json!("Use the November report");
        assert!(
            call(
                &s,
                &format!("replace-{suffix}"),
                "memory.replace",
                input,
                cx
            )
            .recv()
            .unwrap()
            .is_ok()
        );
        let read = call(
            &s,
            &format!("updated-{suffix}"),
            "memory.read",
            extra.clone(),
            cx,
        )
        .recv()
        .unwrap()
        .unwrap();
        assert!(read["text"].as_str().unwrap().contains("November report"));
        let mut input = extra;
        input["find"] = json!("November report");
        assert!(
            call(&s, &format!("remove-{suffix}"), "memory.remove", input, cx)
                .recv()
                .unwrap()
                .is_ok()
        );
    }
}

#[gpui::test]
fn memory_settings_refresh_external_edits_on_tick(cx: &mut TestAppContext) {
    let s = setup(cx);
    mono::memory::edit(&s.root, &s.id, None, |_| {
        Ok("- 2026-11-07 · New external fact\n".into())
    })
    .unwrap();
    let window: gpui::AnyWindowHandle = s.window.into();
    window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = Some(SettingsSection::Memory);
                m.tick(window, cx);
            });
        })
        .unwrap();
    cx.run_until_parked();
    s.manager.read_with(cx, |m, _| {
        assert!(
            m.files
                .as_ref()
                .unwrap()
                .memory
                .contains("New external fact")
        )
    });
}

#[gpui::test]
fn overview_activity_returns_to_its_previous_tab_and_uses_the_shared_reveal(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    s.window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.select_overview(OverviewTab::Documents, cx);
                assert!(m.overview_reveal.is_some());
                m.select_overview(OverviewTab::Activity, cx);
                assert!(m.overview_previous == OverviewTab::Documents);
                m.select_overview(m.overview_previous, cx);
                assert!(m.overview_tab == OverviewTab::Documents);
                cx.set_reduce_motion(true);
                assert!(
                    m.overview_reveal
                        .as_ref()
                        .unwrap()
                        .progress(OverviewTab::Documents, window, cx)
                        .is_none()
                );
            });
        })
        .unwrap();
}

#[gpui::test]
fn floating_chat_opens_after_releasing_the_manager_and_shares_the_runtime(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.open_floating(s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.open_floating(s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let manager = s.manager.read(cx);
        assert_eq!(manager.floating.len(), 1);
        assert_eq!(manager.chats.len(), 1);
        assert!(manager.chats.contains_key(&s.id));
        // The panel owns the chat; the workspace draws a placeholder rather
        // than the same transcript in a second window.
        assert!(manager.floating_shows(&s.id, cx));
    });
    cx.update(|cx| {
        s.manager
            .update(cx, |m, cx| m.hide_floating(&s.id.clone(), cx))
    });
    cx.run_until_parked();
    cx.update(|cx| assert!(!s.manager.read(cx).floating_shows(&s.id, cx)));
}

#[gpui::test]
fn floating_chat_leaves_the_workspace_selection_alone(cx: &mut TestAppContext) {
    let s = setup(cx);
    let other = cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            let before = m.active.clone();
            let id = m.create_floating(cx).unwrap();
            assert_eq!(
                m.active, before,
                "a panel's new bot stays out of the workspace"
            );
            assert!(m.roster.get(&id).unwrap().is_bot());
            m.open_floating(id.clone(), cx);
            assert_eq!(m.active, before);
            id
        })
    });
    cx.run_until_parked();
    cx.update(|cx| {
        let manager = s.manager.read(cx);
        assert!(manager.floating_shows(&other, cx));
        assert!(!manager.floating_shows(&s.id, cx));
    });
}

#[gpui::test]
fn note_actions_preserve_tags_when_updating_the_body(cx: &mut TestAppContext) {
    let s = setup(cx);
    let note = call(
        &s,
        "note-create",
        "notes.write",
        json!({"title":"QA note","body":"First","tags":["work","mono"]}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let updated = call(
        &s,
        "note-update",
        "notes.write",
        json!({"id":note["id"],"title":"QA note","body":"Second"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(updated["tags"], json!(["work", "mono"]));
    assert_eq!(updated["body"], "Second");
}
#[gpui::test]
fn identity_and_context_editors_survive_switches_and_save_to_their_own_files(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.settings = Some(SettingsSection::Soul);
            let mono = m.current().unwrap().clone();
            let _ = m.soul_page(&mono, cx);
            let key = format!("file:{}:SOUL.md", s.id);
            m.fields[&key].update(cx, |f, cx| {
                f.set_text("# Soul\nKeep replies concise.\n".into(), cx)
            });
            m.save_open_file(cx);
        })
    });
    cx.run_until_parked();
    assert_eq!(
        mono::files::read(&s.root, &s.id, None, "SOUL.md")
            .unwrap()
            .text
            .as_deref(),
        Some("# Soul\nKeep replies concise.\n")
    );
    let saved = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert_eq!(saved.monos.len(), 1);
    assert_eq!(saved.monos[0].session_id, Some(s.session));
    assert!(saved.hidden_sessions.contains(&s.session));
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager
                .update(cx, |m, cx| m.create(vec![s.project.clone()], w, cx))
        })
        .unwrap();
    cx.run_until_parked();
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            let mono = m.current().unwrap().clone();
            assert_ne!(mono.id, s.id);
            assert_ne!(mono.mascot, m.roster.get(&s.id).unwrap().mascot);
            m.settings = Some(SettingsSection::Soul);
            let _ = m.soul_page(&mono, cx);
            let key = format!("file:{}:SOUL.md", mono.id);
            assert!(
                !m.fields[&key]
                    .read(cx)
                    .content
                    .contains("Keep replies concise")
            );
        })
    });
}
#[gpui::test]
fn app_receipt_lookup_does_not_wait_for_storage_on_the_ui_thread(cx: &mut TestAppContext) {
    let s = setup(cx);
    let store = s.store.clone();
    let (held, acquired) = mpsc::sync_channel(0);
    let (release, released) = mpsc::channel();
    let holder = std::thread::spawn(move || {
        let _guard = store.lock().unwrap();
        held.send(()).unwrap();
        // A regression must fail rather than hang the whole test suite.
        released.recv_timeout(Duration::from_secs(2)).is_ok()
    });
    acquired.recv().unwrap();
    let (reply, response) = mpsc::channel();
    let call = mono::bridge::Call {
        session: s.session,
        request_id: "contended-lookup".into(),
        action: "sessions.list".into(),
        input: json!({}),
        reply,
    };
    s.window
        .update(cx, |_, window, cx| {
            s.manager
                .update(cx, |manager, cx| manager.app_call(call, window, cx));
        })
        .unwrap();
    assert!(response.try_recv().is_err());
    let _ = release.send(());
    assert!(
        holder.join().unwrap(),
        "app request blocked the UI waiting for the store"
    );
    cx.run_until_parked();
    assert!(response.try_recv().unwrap().is_ok());
}

#[gpui::test]
fn durable_app_receipts_survive_cache_eviction_and_invalid_receipts_do_not_repeat_mutations(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    let input = json!({"name":"Digest","instructions":"Summarize changes","schedule":{"kind":"daily","time":"09:00"}});
    let first = call(&s, "durable-habit", "habits.add", input.clone(), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    s.manager.update(cx, |m, _| m.app_results.clear());
    let retry = call(&s, "durable-habit", "habits.add", input, cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
    assert!(
        call(
            &s,
            "durable-habit",
            "habits.add",
            json!({"name":"different"}),
            cx
        )
        .try_recv()
        .unwrap()
        .is_err()
    );
    let key = format!(
        "mono-app-receipt:{}:{}",
        s.session,
        mono::files::content_hash("corrupt-habit")
    );
    locked(&s.store)
        .unwrap()
        .save_setting(&key, "invalid receipt")
        .unwrap();
    let error = call(&s, "corrupt-habit", "habits.add", json!({"name":"Another","instructions":"Summarize changes","schedule":{"kind":"daily","time":"09:00"}}), cx).try_recv().unwrap().unwrap_err();
    assert!(error.contains("Could not read app result"));
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
}

#[gpui::test]
fn app_actions_are_project_scoped_and_mutation_retries_are_idempotent(cx: &mut TestAppContext) {
    let s = setup(cx);
    assert!(
        call(
            &s,
            "outside",
            "sessions.list",
            json!({"project":"/not-assigned"}),
            cx
        )
        .try_recv()
        .unwrap()
        .is_err()
    );
    let input = json!({"name":"Morning digest","instructions":"Summarize changes without editing","schedule":{"kind":"daily","time":"09:00"}});
    let first = call(&s, "habit", "habits.add", input.clone(), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    let retry = call(&s, "habit", "habits.add", input, cx)
        .try_recv()
        .unwrap()
        .unwrap();
    assert_eq!(first, retry);
    assert_eq!(habits::load(&s.root, &s.id).unwrap().len(), 1);
    assert!(
        call(&s, "habit", "habits.add", json!({"name":"different"}), cx)
            .try_recv()
            .unwrap()
            .is_err()
    );
    let start=call(&s,"draft","sessions.start",json!({"project":s.project,"prompt":"Review the project","title":"Review draft","draft":true}),cx).try_recv().unwrap().unwrap();
    let child = start["sessionId"].as_i64().unwrap();
    cx.update(|cx| {
        let m = s.manager.read(cx);
        assert_eq!(m.roster.get(&s.id).unwrap().launches.len(), 1);
        let owner = m.owner.upgrade().unwrap();
        let view = owner.read(cx).mono_session_entity(child).unwrap();
        assert!(view.read(cx).has_saved_draft());
        assert_eq!(
            locked(&s.store)
                .unwrap()
                .follow_ups(child)
                .unwrap()
                .draft
                .unwrap()
                .text,
            "Review the project"
        );
        assert!(!view.read(cx).busy);
        assert!(m.hidden_session(child));
    });
}
#[gpui::test]
fn mono_launches_ignore_tab_reveal_and_keep_the_current_chat(cx: &mut TestAppContext) {
    let s = setup(cx);
    let before = s
        .window
        .update(cx, |shell, _, _| shell.workspace.open_sessions())
        .unwrap();
    for show in [false, true] {
        s.manager
            .update(cx, |m, cx| m.sidebar_preferences(&s.id, show, false, cx));
        let started = call(
            &s,
            if show {
                "listed-reveal"
            } else {
                "hidden-reveal"
            },
            "sessions.start",
            json!({"project":s.project,"prompt":"Test quietly","draft":true,"reveal":true}),
            cx,
        )
        .try_recv()
        .unwrap()
        .unwrap();
        let child = started["sessionId"].as_i64().unwrap();
        s.window
            .update(cx, |shell, _, cx| {
                assert_eq!(
                    shell.workspace.open_sessions(),
                    before,
                    "a Mono launch must not create or select a tab"
                );
                assert!(
                    shell.mono_session_entity(child).is_some(),
                    "the background session still exists"
                );
                let m = s.manager.read(cx);
                assert_eq!(m.active.as_deref(), Some(s.id.as_str()));
                assert_eq!(m.hidden_session(child), !show);
            })
            .unwrap();
    }
}

#[gpui::test]
fn internal_completion_queue_stays_hidden_until_delivery_fails(cx: &mut TestAppContext) {
    let s = setup(cx);
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                m.roster.get_mut(&s.id).unwrap().outbox.push(Message {
                    id: "completion".into(),
                    text: "[Delegated work completed]".into(),
                    internal: true,
                    sent_at: None,
                    error: None,
                    attachments: Vec::new(),
                });
                // Keep the notification queued without starting a provider.
                m.configure_chat(&s.id, cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-internal-queue").is_none(),
        "a routine completion must not flash a Pending card in the composer"
    );
    s.manager.update(visual, |m, cx| {
        m.roster.get_mut(&s.id).unwrap().outbox[0].error = Some("Delivery failed".into());
        m.configure_chat(&s.id, cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-internal-queue").is_some(),
        "a failed notification keeps its retry and discard controls"
    );
}

#[gpui::test]
fn sidebar_hidden_launches_still_list_in_the_mono_panel_after_a_restart(cx: &mut TestAppContext) {
    let s = setup(cx);
    let started = call(
        &s,
        "hidden-launch",
        "sessions.start",
        json!({"project":s.project,"prompt":"Review quietly","title":"Quiet review","draft":true}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let child = started["sessionId"].as_i64().unwrap();
    let owner = cx.update(|cx| s.manager.read(cx).owner.upgrade().unwrap());
    cx.update(|cx| {
        assert!(s.manager.read(cx).hidden_session(child));
        owner.update(cx, |shell, _| shell.mono_forget_session(child));
        assert!(
            !owner
                .read(cx)
                .mono_session_views(cx)
                .iter()
                .any(|v| v.id == child)
        );
    });
    s.manager.update(cx, |m, cx| m.configure_chat(&s.id, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        let views = owner.read(cx).mono_session_views(cx);
        let view = views
            .iter()
            .find(|v| v.id == child)
            .expect("the launch lists again");
        assert!(view.title.contains("Quiet review"), "{}", view.title);
        assert!(
            owner
                .update(cx, |shell, cx| shell.mono_ensure_session(child, cx))
                .is_some()
        );
        assert!(s.manager.read(cx).hidden_session(child));
    });
}
#[gpui::test]
fn mono_app_launch_visibility_can_be_toggled_and_survives_roster_reload(cx: &mut TestAppContext) {
    let s = setup(cx);
    let started = call(
        &s,
        "visibility-draft",
        "sessions.start",
        json!({"project":s.project,"prompt":"Review quietly","draft":true}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let child = started["sessionId"].as_i64().unwrap();
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(child)));
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, true, true, cx));
    cx.run_until_parked();
    cx.update(|cx| {
        assert!(!s.manager.read(cx).hidden_session(child));
        assert!(s.manager.read(cx).is_mono_session(s.session));
    });
    assert!(
        mono::Roster::load(&locked(&s.store).unwrap())
            .unwrap()
            .get(&s.id)
            .unwrap()
            .show_started_sessions_in_sidebar
    );
    s.manager
        .update(cx, |m, cx| m.sidebar_preferences(&s.id, false, false, cx));
    cx.run_until_parked();
    cx.update(|cx| assert!(s.manager.read(cx).hidden_session(child)));
    assert!(
        !mono::Roster::load(&locked(&s.store).unwrap())
            .unwrap()
            .get(&s.id)
            .unwrap()
            .show_started_sessions_in_sidebar
    );
}
#[test]
fn agent_requests_wait_for_a_separate_turn_while_normal_messages_can_steer() {
    let mut message = Message {
        id: "request".into(),
        text: monocode_core::mono::agents::agent_message("Principal", "principal", "Do work"),
        sent_at: None,
        internal: false,
        error: None,
        attachments: Vec::new(),
    };
    assert!(can_dispatch(&message, false));
    assert!(!can_dispatch(&message, true));
    message.text = "A follow-up from the user".into();
    assert!(can_dispatch(&message, true));
    message.internal = true;
    assert!(!can_dispatch(&message, true));
    message.error = Some("Retry required".into());
    assert!(!can_dispatch(&message, false));
}

#[gpui::test]
fn queued_messages_are_saved_before_dispatch_and_do_not_run_while_blocked(cx: &mut TestAppContext) {
    let s = setup(cx);
    let message = mono::new_id();
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            m.roster.get_mut(&s.id).unwrap().outbox.push(Message {
                sent_at: None,
                id: message.clone(),
                text: "Keep this queued".into(),
                internal: false,
                error: Some("Retry required".into()),
                attachments: Vec::new(),
            });
            m.save(cx);
            m.configure_chat(&s.id, cx);
        })
    });
    cx.run_until_parked();
    let saved = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert_eq!(saved.monos[0].outbox[0].id, message);
    assert_eq!(
        locked(&s.store)
            .unwrap()
            .load(s.session)
            .unwrap()
            .rows
            .len(),
        0
    );
}

#[gpui::test]
fn a_failed_queued_message_does_not_hold_back_the_next(cx: &mut TestAppContext) {
    let s = setup(cx);
    let failed = mono::new_id();
    let clean = mono::new_id();
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            let outbox = &mut m.roster.get_mut(&s.id).unwrap().outbox;
            for (id, text, error) in [
                (
                    failed.clone(),
                    "This send failed",
                    Some("Retry required".to_string()),
                ),
                (clean.clone(), "This one must still go", None),
            ] {
                outbox.push(Message {
                    sent_at: None,
                    id,
                    text: text.into(),
                    internal: false,
                    error,
                    attachments: Vec::new(),
                });
            }
            m.save(cx);
            m.configure_chat(&s.id, cx);
        })
    });
    cx.run_until_parked();
    s.manager.read_with(cx, |m, cx| {
        assert_eq!(
            m.chats[&s.id].read(cx).settings.mono_message_id.as_deref(),
            Some(clean.as_str()),
            "the message behind a failed send still dispatches"
        );
        let outbox = &m.roster.get(&s.id).unwrap().outbox;
        assert!(
            outbox
                .iter()
                .any(|entry| entry.id == failed && entry.error.is_some()),
            "the failed one keeps its Retry/Discard card"
        );
    });
}

#[gpui::test]
fn chat_composer_matches_tauri_and_opens_the_model_list(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.dismiss_settings(w, cx));
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let field = visual.debug_bounds("mono-composer").unwrap();
    let send = visual.debug_bounds("mono-send").unwrap();
    let attach = visual.debug_bounds("mono-attach").unwrap();
    assert!(visual.debug_bounds("mono-model").is_none());
    // A click anywhere in the box, beside the field included, focuses it.
    let toolbar = visual.debug_bounds("mono-composer-toolbar").unwrap();
    for point in [
        toolbar.center(),
        gpui::point(send.left() - px(1.), send.center().y),
    ] {
        handle.update(visual, |_, w, cx| w.blur(cx)).unwrap();
        visual.simulate_click(point, Default::default());
        visual.run_until_parked();
        let chat = visual.update(|_, cx| s.manager.read(cx).chat().unwrap());
        assert!(
            handle
                .update(visual, |_, w, cx| chat
                    .read(cx)
                    .input
                    .focus_handle(cx)
                    .is_focused(w))
                .unwrap(),
            "a click at {point:?} focuses the composer"
        );
    }
    // MonoComposer's box: one line with attach, emoji, the field, meter,
    // dictation and send, its 5px padding centring send. The model is
    // configured in agent settings (⚙️), not in the chat prompt.
    assert_eq!(field.size.height, px(38.));
    assert_eq!(field.bottom() - send.bottom(), px(6.));
    assert!(field.contains(&send.center()));
    assert!(visual.debug_bounds("mono-access").is_none());
    // Inside the field: attach on the left, emoji next, context meter before send.
    assert!(field.contains(&attach.center()));
    assert_eq!(attach.size, gpui::size(px(26.), px(26.)));
    let emoji = visual.debug_bounds("mono-emoji").unwrap();
    assert!(field.contains(&emoji.center()));
    assert!(attach.right() <= emoji.left());
    let meter = visual.debug_bounds("mono-context-meter").unwrap();
    assert!(field.contains(&meter.center()));
    assert!(meter.right() <= send.left());
    let chat = visual.update(|_, cx| s.manager.read(cx).chat().unwrap());
    for draft in [
        "".to_owned(),
        "First line\nSecond line".to_owned(),
        "A message that wraps automatically. ".repeat(40),
        "".to_owned(),
    ] {
        chat.update(visual, |chat, cx| {
            chat.input.update(cx, |input, cx| input.set_text(draft, cx));
        });
        visual.run_until_parked();
        visual.refresh().unwrap();
        let field = visual.debug_bounds("mono-composer").unwrap();
        let send = visual.debug_bounds("mono-send").unwrap();
        let attach = visual.debug_bounds("mono-attach").unwrap();
        assert!(field.contains(&send.center()));
        assert!(field.contains(&attach.center()));
    }
    handle
        .update(visual, |_, w, cx| {
            chat.update(cx, |c, cx| c.toggle_model_menu(w, cx))
        })
        .unwrap();
    visual.run_until_parked();
    visual.update(|_, cx| {
        let (models, menu) = chat.read(cx).mono_menus_open();
        assert!(models, "the trigger opens the model list itself");
        assert!(!menu, "not the session composer's settings menu");
    });
}

#[gpui::test]
fn details_tabs_share_the_session_titlebar_in_both_layouts(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        crate::shell::bind_keys(cx);
    });
    let session = s
        .window
        .update(cx, |shell, w, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.focus_active(w, cx);
                cx.notify();
            });
            shell.workspace.active.unwrap()
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    for classic in [false, true] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.new_layout_boxy = classic;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        let panel = visual.debug_bounds("right-panel-frame").unwrap();
        let tabs = visual.debug_bounds("mono-overview-tabs").unwrap();
        let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
        let title_tabs = visual.debug_bounds("title-tab-strip").unwrap();
        assert!((tabs.top() - title_tabs.top()).abs() <= px(0.5));
        assert_eq!(tabs.size.height, px(TITLE_BAR_HEIGHT));
        assert_eq!(
            tabs.left(),
            panel.left() + px(if classic { 6. } else { 7. })
        );
        let activity = visual.debug_bounds("mono-activity").unwrap();
        let search = visual.debug_bounds("titlebar-file-search").unwrap();
        assert_eq!(search.left() - activity.right(), px(6.));
        assert_eq!(toggle.left() - search.right(), px(4.));
        assert!(tabs.bottom() <= panel.top() + px(if classic { TITLE_BAR_HEIGHT } else { 0. }));
        assert_eq!(
            visual.debug_bounds("mono-dashboard").unwrap().top(),
            panel.top() + px(if classic { TITLE_BAR_HEIGHT } else { 0. })
        );
        assert!(
            visual.debug_bounds("mono-overview-toggle").is_none(),
            "only the shell owns the panel toggle"
        );
        assert!(title_tabs.right() <= tabs.left());
        s.window
            .update(visual, |shell, w, cx| shell.open(session, w, cx))
            .unwrap();
        visual.run_until_parked();
        if !s
            .window
            .update(visual, |shell, _, _| shell.workspace.right_panel_open)
            .unwrap()
        {
            let button = visual.debug_bounds("toggle-right-panel").unwrap();
            visual.simulate_click(button.center(), Default::default());
            visual.run_until_parked();
        }
        assert_eq!(
            visual.debug_bounds("toggle-right-panel").unwrap(),
            toggle,
            "the toggle stays put when changing tab types"
        );
        let selector = if classic {
            "right-panel-explorer"
        } else {
            "project-tabs-separator"
        };
        assert_eq!(
            visual.debug_bounds(selector).unwrap().center().y,
            tabs.center().y
        );
        s.window
            .update(visual, |_, w, cx| {
                s.manager.update(cx, |m, cx| m.open(&s.id, w, cx))
            })
            .unwrap();
        visual.run_until_parked();
    }
}

#[gpui::test]
fn titlebar_details_tabs_switch_pages_and_keep_activity_and_habit_actions(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 400.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    for (selector, tab) in [
        ("mono-overview-Documents", OverviewTab::Documents),
        ("mono-overview-Habits", OverviewTab::Habits),
    ] {
        let button = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(button.center(), Default::default());
        visual.run_until_parked();
        assert!(s.manager.read_with(visual, |m, _| m.overview_tab == tab));
        assert!(visual.debug_bounds(selector).unwrap().size.width > px(26.));
    }
    let add = visual.debug_bounds("mono-new-habit").unwrap();
    visual.simulate_click(add.center(), Default::default());
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| matches!(m.route, Some(Route::NewHabit)))
    );
    assert!(visual.debug_bounds("mono-overview-tabs").is_some());
    let sessions = visual.debug_bounds("mono-overview-Sessions").unwrap();
    visual.simulate_click(sessions.center(), Default::default());
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()
        && m.overview_tab == OverviewTab::Sessions));
    for expected in [OverviewTab::Activity, OverviewTab::Sessions] {
        let activity = visual.debug_bounds("mono-activity").unwrap();
        visual.simulate_click(activity.center(), Default::default());
        visual.run_until_parked();
        assert!(
            s.manager
                .read_with(visual, |m, _| m.overview_tab == expected)
        );
    }
}

#[gpui::test]
fn titlebar_divider_resizes_mono_details_and_closed_panels_keep_the_corner(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |shell, _, cx| {
            // The titlebar divider belongs to the rounded frame.
            shell.workspace.new_layout_boxy = false;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    for width in [220., 280., 400., 500.] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.right_panel_width = width;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert_eq!(
            visual
                .debug_bounds("project-tabs-separator")
                .unwrap()
                .left(),
            visual.debug_bounds("right-panel-frame").unwrap().left()
        );
        let activity = visual.debug_bounds("mono-activity").unwrap();
        let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
        assert!(
            activity.right() < toggle.left(),
            "Mono controls fit the session sidebar's minimum width"
        );
    }
    let start = visual
        .debug_bounds("titlebar-right-panel-resize")
        .unwrap()
        .center();
    let end = start - gpui::point(px(40.), px(0.));
    visual.simulate_mouse_down(start, MouseButton::Left, Default::default());
    visual.simulate_mouse_move(end, MouseButton::Left, Default::default());
    visual.simulate_mouse_up(end, MouseButton::Left, Default::default());
    visual.run_until_parked();
    s.window
        .update(visual, |shell, _, _| {
            assert_eq!(shell.workspace.right_panel_width, 540.);
        })
        .unwrap();
    assert_eq!(
        visual
            .debug_bounds("project-tabs-separator")
            .unwrap()
            .left(),
        visual.debug_bounds("right-panel-frame").unwrap().left()
    );
    for classic in [false, true] {
        s.window
            .update(visual, |shell, _, cx| {
                shell.workspace.new_layout_boxy = classic;
                shell.git.update(cx, |g, cx| g.set_project("~".into(), cx));
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        let open = visual.debug_bounds("toggle-right-panel").unwrap();
        visual.simulate_click(open.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("mono-overview-tabs").is_none());
        assert!(visual.debug_bounds("mono-overview-panel").is_none());
        assert_eq!(visual.debug_bounds("toggle-right-panel").unwrap(), open);
        visual.simulate_click(open.center(), Default::default());
        visual.run_until_parked();
        assert!(visual.debug_bounds("mono-overview-tabs").is_some());
    }
}

#[gpui::test]
fn opening_a_session_card_uses_the_shared_sidebar_state(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let session = s
        .window
        .update(cx, |shell, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            });
            shell.workspace.active.unwrap()
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("right-panel-frame").is_none());
    s.window
        .update(visual, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.card_action(&s.id, &json!({"type":"session", "sessionId":session}), cx);
            })
        })
        .unwrap();
    visual.run_until_parked();
    let details = visual.debug_bounds("right-panel-frame").unwrap();
    assert!(s.manager.read_with(
        visual,
        |m, _| matches!(m.route, Some(Route::Session(id)) if id == session)
    ));
    s.window
        .update(visual, |shell, w, cx| {
            assert!(shell.workspace.right_panel_open);
            shell.open(session, w, cx);
        })
        .unwrap();
    visual.run_until_parked();
    assert_eq!(visual.debug_bounds("right-panel-frame").unwrap(), details);
}

#[gpui::test]
fn a_session_open_in_mono_details_keeps_its_history(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let session = s
        .window
        .update(cx, |shell, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                cx.notify();
            });
            shell.workspace.active.unwrap()
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.window
        .update(visual, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.card_action(&s.id, &json!({"type":"session", "sessionId":session}), cx);
            })
        })
        .unwrap();
    visual.run_until_parked();
    // The details panel shows this session, so memory trimming must not
    // release its transcript while the Mono view is active.
    s.window
        .update(visual, |shell, _, cx| {
            assert!(shell.workspace.right_panel_open);
            let chat = shell.mono_ensure_session(session, cx).unwrap();
            assert!(chat.read(cx).history_loaded());
            assert!(!chat.update(cx, |chat, cx| chat.unload_history(cx)));
        })
        .unwrap();
}

#[gpui::test]
fn group_dialog_scrolls_and_keeps_actions_visible(cx: &mut TestAppContext) {
    let s = setup(cx);
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.simulate_resize(gpui::size(px(800.), px(600.)));
    s.window
        .update(visual, |_, window, cx| {
            s.manager
                .update(cx, |m, cx| m.open_group_dialog(None, window, cx));
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();

    let dialog = visual.debug_bounds("group-dialog").unwrap();
    let body = visual.debug_bounds("group-dialog-body").unwrap();
    let footer = visual.debug_bounds("group-dialog-footer").unwrap();
    let habits = visual.debug_bounds("group-dialog-habits-toggle").unwrap();
    assert!(dialog.bottom() <= px(600.));
    assert!(footer.bottom() <= dialog.bottom());
    assert!(footer.top() >= body.bottom());

    visual.simulate_event(gpui::ScrollWheelEvent {
        position: gpui::point(body.right() - px(4.), body.center().y),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-3000.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    visual.refresh().unwrap();

    let scrolled_habits = visual.debug_bounds("group-dialog-habits-toggle").unwrap();
    assert!(scrolled_habits.top() < habits.top());
    assert!(scrolled_habits.bottom() <= body.bottom());
    assert_eq!(visual.debug_bounds("group-dialog-footer").unwrap(), footer);
    visual.simulate_click(scrolled_habits.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("group-dialog-habits-undo").is_some());
}

#[gpui::test]
fn long_habit_draft_keeps_fields_and_schedule_inside_the_scrolling_page(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        input::bind(cx);
        crate::popover::bind(cx);
    });
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 220.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.route = Some(Route::NewHabit);
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.manager.update(visual, |m, cx| {
        m.fields["habit-name"].update(cx, |f, cx| {
            f.set_text(
                "A long but valid habit name that fills the narrow panel".into(),
                cx,
            )
        });
        m.fields["habit-instructions"].update(cx, |f, cx| {
            f.set_text(
                "Report useful updates on the current project.\n".repeat(65),
                cx,
            )
        });
        m.fields["habit-kind"].update(cx, |f, cx| f.set_text("weekly".into(), cx));
        cx.notify();
    });
    visual.run_until_parked();
    let body = visual.debug_bounds("mono-habit-editor").unwrap();
    let name = visual.debug_bounds("mono-habit-name-field").unwrap();
    let schedule = visual.debug_bounds("mono-habit-schedule").unwrap();
    let prompt = visual.debug_bounds("mono-habit-prompt-field").unwrap();
    let create = visual.debug_bounds("mono-habit-save").unwrap();
    assert!(name.size.height >= px(36.));
    assert!(schedule.top() >= name.bottom());
    assert!(prompt.top() > schedule.bottom());
    assert!(prompt.size.height >= px(300.));
    assert!(prompt.size.height <= px(328.));
    for bounds in [name, schedule, prompt] {
        assert!(bounds.left() >= body.left());
        assert!(bounds.right() <= body.right() + px(1.));
    }
    assert!(create.bottom() <= body.top());
    let day = visual.debug_bounds("mono-habit-weekday-picker").unwrap();
    assert!(day.left() >= schedule.left());
    assert!(day.right() <= schedule.right() + px(1.));
    visual.simulate_click(day.center(), Default::default());
    visual.run_until_parked();
    // Escape returns focus to the trigger; arrow keys reopen the list with
    // the current weekday highlighted, and Enter retains that selection.
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_none()));
    visual.simulate_keystrokes("down");
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_some()));
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    assert_eq!(
        s.manager
            .read_with(visual, |m, cx| m.value("habit-weekday", cx)),
        "1"
    );
    assert!(s.manager.read_with(visual, |m, _| m.picker.is_none()));
    visual.simulate_click(day.center(), Default::default());
    visual.run_until_parked();
    let saturday = visual.debug_bounds("menu-action-6").unwrap();
    visual.simulate_click(saturday.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        s.manager
            .read_with(visual, |m, cx| m.value("habit-weekday", cx)),
        "6"
    );
    // Invalid schedules keep the full draft in place instead of navigating away.
    s.manager.update(visual, |m, cx| {
        m.fields["habit-time"].update(cx, |f, cx| f.set_text("25:00".into(), cx));
    });
    visual.run_until_parked();
    visual.simulate_click(create.center(), Default::default());
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| m.route == Some(Route::NewHabit))
    );
    assert!(s.manager.read_with(visual, |m, cx| {
        m.fields["habit-instructions"].read(cx).content.len() > 2000
    }));
    assert!(habits::load(&s.root, &s.id).unwrap().is_empty());
    s.manager.update(visual, |m, cx| {
        m.fields["habit-time"].update(cx, |f, cx| f.set_text("09:00".into(), cx));
    });
    visual.run_until_parked();
    visual.simulate_click(create.center(), Default::default());
    visual.run_until_parked();
    let saved = habits::load(&s.root, &s.id).unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].instructions.len() > 2000);
    assert_eq!(saved[0].schedule.schedule_kind, "weekly");
    assert_eq!(saved[0].schedule.day_of_week, 6);
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
}

#[gpui::test]
fn habit_detail_preserves_long_content_history_and_actions_without_edit_fields(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mut h = Habit::new(
        "A long habit title that should truncate before the action buttons".into(),
        "Review changes and report anything that needs attention.".into(),
        habits::Schedule::default(),
        monocode_core::time::now_ms(),
    )
    .unwrap();
    h.id = "layout-history".into();
    h.runs.push(habits::Run {
        at: monocode_core::time::now_ms() - 60_000,
        duration_ms: 72_000,
        outcome: "posted".into(),
        report: Some("**Result**\n\nThe project is ready to review.".into()),
        status: None,
        error: None,
    });
    habits::update(&s.root, &s.id, |all| {
        all.push(h.clone());
        Ok(())
    })
    .unwrap();
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 280.;
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.habits = vec![h.clone()];
                m.route = Some(Route::Habit(h.id.clone()));
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    assert!(
        s.manager
            .read_with(visual, |m, _| !m.fields.contains_key("habit-name"))
    );
    let body = visual.debug_bounds("mono-habit-editor").unwrap();
    let description = visual.debug_bounds("mono-habit-instructions").unwrap();
    let row_id = format!("mono-habit-history-{}-{}", h.id, h.runs[0].at);
    let row = visual
        .debug_bounds(Box::leak(row_id.into_boxed_str()))
        .unwrap();
    assert!(row.top() > description.bottom());
    assert!(description.right() <= body.right());
    for selector in ["mono-habit-run", "mono-habit-toggle", "mono-habit-delete"] {
        let action = visual.debug_bounds(selector).unwrap();
        assert!(action.bottom() <= body.top());
        assert!(action.right() <= body.right());
    }
    visual.simulate_click(row.center(), Default::default());
    visual.run_until_parked();
    assert!(
        visual
            .debug_bounds(Box::leak(
                format!("mono-habit-report-{}-{}", h.id, h.runs[0].at).into_boxed_str()
            ))
            .is_some()
    );
    let toggle = visual.debug_bounds("mono-habit-toggle").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    assert!(!habits::load(&s.root, &s.id).unwrap()[0].enabled);
    assert!(
        s.manager
            .read_with(visual, |m, _| m.route == Some(Route::Habit(h.id.clone())))
    );
    let remove = visual.debug_bounds("mono-habit-delete").unwrap();
    visual.simulate_click(remove.center(), Default::default());
    visual.run_until_parked();
    assert!(habits::load(&s.root, &s.id).unwrap().is_empty());
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
}

#[gpui::test]
fn new_habit_click_transfers_composer_focus_after_the_animated_page_mounts(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(false));
    s.window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.route = None;
                m.focus_active(window, cx);
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    s.window
        .update(visual, |_, window, cx| {
            s.manager.read_with(cx, |m, cx| {
                assert!(
                    m.chat()
                        .unwrap()
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(window)
                );
            });
        })
        .unwrap();
    let advance_focus_frame = |visual: &mut gpui::VisualTestContext| {
        visual
            .update_window(s.window.into(), |_, window, cx| {
                // Use the untyped window context: callbacks and drawing may
                // update the root Shell, so it must not already be borrowed.
                // Parking executor tasks does not deliver platform frame callbacks.
                // Flush the requested next frame before drawing its focused field.
                assert!(window.simulate_next_frame(cx) > 0);
                window.draw(cx).clear(cx);
            })
            .unwrap();
        visual.run_until_parked();
    };
    // Follow the actual header action, with the old composer still focused.
    let add = visual.debug_bounds("mono-new-habit").unwrap();
    visual.simulate_click(add.center(), Default::default());
    visual.run_until_parked();
    advance_focus_frame(visual);
    s.window
        .update(visual, |_, window, cx| {
            s.manager.read_with(cx, |m, cx| {
                assert_eq!(m.route, Some(Route::NewHabit));
                assert!(m.fields["habit-name"].focus_handle(cx).is_focused(window));
                assert!(
                    !m.chat()
                        .unwrap()
                        .read(cx)
                        .input
                        .focus_handle(cx)
                        .is_focused(window)
                );
            });
        })
        .unwrap();
    let page = "mono-detail-page-new-habit";
    assert!(visual.debug_bounds(page).is_some());
    // Complete the real page transition, then leave/reopen through its controls.
    for reopen in [false, true] {
        visual
            .update_window(s.window.into(), |_, window, cx| {
                let details = s.manager.read(cx).details.clone();
                details.update(cx, |v, cx| v.elapsed(std::time::Duration::from_secs(1), cx));
                window.draw(cx).clear(cx);
            })
            .unwrap();
        let back = visual.debug_bounds("mono-page-back").unwrap();
        visual.simulate_click(back.center(), Default::default());
        visual.run_until_parked();
        assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
        if !reopen {
            let add = visual.debug_bounds("mono-new-habit").unwrap();
            visual.simulate_click(add.center(), Default::default());
            visual.run_until_parked();
            advance_focus_frame(visual);
            s.window
                .update(visual, |_, window, cx| {
                    s.manager.read_with(cx, |m, cx| {
                        assert!(m.fields["habit-name"].focus_handle(cx).is_focused(window))
                    });
                })
                .unwrap();
        }
    }
}

#[gpui::test]
fn mono_hover_card_closes_after_crossing_its_border(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.close(cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let row_selector: &'static str = Box::leak(format!("mono-rail-{}", s.id).into_boxed_str());
    let row = visual.debug_bounds(row_selector).unwrap();
    visual.simulate_mouse_move(row.center(), None, Default::default());
    visual.run_until_parked();
    let card = visual
        .debug_bounds("mono-preview")
        .expect("row opens its preview");
    visual.simulate_mouse_move(card.center(), None, Default::default());
    visual.run_until_parked();
    let entered = s
        .manager
        .read_with(visual, |m, _| m.mono_preview.diagnostic());
    // The popover's 1px frame is outside its content's hover listener.
    let border = gpui::point(card.left() - px(0.5), card.center().y);
    visual.simulate_mouse_move(border, None, Default::default());
    visual.run_until_parked();
    let framed = s
        .manager
        .read_with(visual, |m, _| m.mono_preview.diagnostic());
    visual.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Default::default());
    visual.run_until_parked();
    let left = s
        .manager
        .read_with(visual, |m, _| m.mono_preview.diagnostic());
    for step in 1..=3 {
        visual.executor().advance_clock(Duration::from_millis(50));
        visual.run_until_parked();
        visual.simulate_mouse_move(
            gpui::point(px(5. + step as f32), px(5.)),
            None,
            Default::default(),
        );
        visual.run_until_parked();
    }
    let settled = s
        .manager
        .read_with(visual, |m, _| m.mono_preview.diagnostic());
    assert!(
        visual.debug_bounds("mono-preview").is_none(),
        "leaving through the frame must dismiss the preview: entered={entered}; frame={framed}; left={left}; settled={settled}"
    );
    s.manager
        .read_with(visual, |m, _| assert!(m.mono_preview.shown.is_none()));
}

#[gpui::test]
fn mono_hover_card_survives_the_gap_but_closes_on_sidebar_scroll_and_blur(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.close(cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    let selector: &'static str = Box::leak(format!("mono-rail-{}", s.id).into_boxed_str());
    let row = visual.debug_bounds(selector).unwrap();
    visual.simulate_mouse_move(row.center(), None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    let card = visual.debug_bounds("mono-preview").unwrap();
    visual.simulate_mouse_move(
        gpui::point(row.right() + px(4.), row.center().y),
        None,
        Default::default(),
    );
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(50));
    visual.simulate_mouse_move(card.center(), None, Default::default());
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(150));
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-preview").is_some(),
        "crossing the gap must let the user read the card"
    );
    visual.simulate_mouse_move(row.center(), None, Default::default());
    visual.simulate_event(gpui::ScrollWheelEvent {
        position: row.center(),
        delta: gpui::ScrollDelta::Pixels(gpui::point(px(0.), px(-40.))),
        modifiers: Default::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-preview").is_none(),
        "scrolling must not leave a detached preview"
    );
    visual.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Default::default());
    visual.simulate_mouse_move(row.center(), None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("mono-preview").is_some());
    s.window
        .update(visual, |_, window, _| window.activate_window())
        .unwrap();
    visual.deactivate_window();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-preview").is_none(),
        "changing windows dismisses the hover card"
    );
}

#[gpui::test]
fn mono_hover_card_switching_rows_cancels_the_old_close_timer(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.manager.update(cx, |m, cx| {
        m.settings = None;
        m.close(cx);
        m.roster.monos.push(Mono {
            id: "hover-second".into(),
            name: "Second agent".into(),
            projects: vec![s.project.clone()],
            ..Default::default()
        });
        cx.notify();
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    let first: &'static str = Box::leak(format!("mono-rail-{}", s.id).into_boxed_str());
    let first = visual.debug_bounds(first).unwrap();
    let second = visual.debug_bounds("mono-rail-hover-second").unwrap();
    visual.simulate_mouse_move(first.center(), None, Default::default());
    visual.run_until_parked();
    visual.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Default::default());
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(60));
    visual.simulate_mouse_move(second.center(), None, Default::default());
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(90));
    visual.run_until_parked();
    s.manager.read_with(visual, |m, _| {
        assert_eq!(m.mono_preview.shown.as_deref(), Some("hover-second"))
    });
    visual.simulate_mouse_move(first.center(), None, Default::default());
    visual.run_until_parked();
    s.manager.read_with(visual, |m, _| {
        assert_eq!(m.mono_preview.shown.as_deref(), Some(s.id.as_str()))
    });
    visual.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Default::default());
    visual.run_until_parked();
    visual.executor().advance_clock(Duration::from_millis(150));
    visual.run_until_parked();
    s.manager
        .read_with(visual, |m, _| assert!(m.mono_preview.shown.is_none()));
}

#[gpui::test]
fn sidebar_mono_only_marks_unread_replies_and_swaps_the_mark_for_options_on_hover(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.close(cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    let unread_selector: &'static str =
        Box::leak(format!("mono-rail-unread-{}", s.id).into_boxed_str());
    let options_selector: &'static str =
        Box::leak(format!("mono-rail-options-{}", s.id).into_boxed_str());
    let row_selector: &'static str = Box::leak(format!("mono-rail-{}", s.id).into_boxed_str());
    let away = gpui::point(px(5.), px(5.));
    visual.simulate_mouse_move(away, None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    let row = visual.debug_bounds(row_selector).unwrap();
    assert!(visual.debug_bounds(unread_selector).is_none());
    assert!(visual.debug_bounds(options_selector).is_none());
    // Check the painted output too: an idle Mono used to paint a gray
    // status dot here, even with no unread reply and no pointer over it.
    s.window
        .update(visual, |_, window, _| {
            let scale = window.scale_factor();
            assert!(!window.painted_quads().iter().any(|quad| {
                let bounds = quad.bounds;
                bounds.left().0 >= f32::from(row.right() - px(28.)) * scale
                    && bounds.right().0 <= f32::from(row.right() - px(4.)) * scale
                    && bounds.top().0 >= f32::from(row.top()) * scale
                    && bounds.bottom().0 <= f32::from(row.bottom()) * scale
                    && (3. * scale..=8. * scale).contains(&bounds.size.width.0)
                    && (3. * scale..=8. * scale).contains(&bounds.size.height.0)
            }));
        })
        .unwrap();

    let post = |role, text: &str, visual: &mut gpui::VisualTestContext| {
        s.window
            .update(visual, |_, _, cx| {
                let chat = s.manager.read(cx).chats[&s.id].clone();
                chat.update(cx, |v, cx| v.mono_post(role, text.into(), cx))
                    .unwrap();
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
    };
    post("user", "My question", visual);
    assert!(visual.debug_bounds(unread_selector).is_none());
    post("assistant", "A new reply", visual);
    let unread = visual
        .debug_bounds(unread_selector)
        .expect("an unseen assistant reply has an accent circle");
    assert_eq!(unread.size, gpui::size(px(6.), px(6.)));
    assert!(visual.debug_bounds(options_selector).is_none());
    let before_hover = visual.debug_bounds(row_selector).unwrap();
    visual.simulate_mouse_move(before_hover.center(), None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds(unread_selector).is_none(),
        "options replace the unread circle on hover"
    );
    let options = visual.debug_bounds(options_selector).unwrap();
    assert_eq!(options.size, gpui::size(px(24.), px(24.)));
    assert_eq!(options.center(), unread.center());
    assert_eq!(visual.debug_bounds(row_selector).unwrap(), before_hover);
    visual.simulate_click(options.center(), Default::default());
    visual.run_until_parked();
    s.manager.read_with(visual, |m, _| {
        assert!(m.menu.is_some(), "the hover action opens Mono options");
        assert!(m.active.is_none(), "options must not open the Mono");
    });

    s.window
        .update(visual, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.menu = None;
                m.open(&s.id, window, cx);
            });
        })
        .unwrap();
    visual.simulate_mouse_move(away, None, Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds(unread_selector).is_none());
    s.window
        .update(visual, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                assert_eq!(
                    m.roster.get(&s.id).unwrap().seen_rows,
                    m.chats[&s.id].read(cx).mono_rows(cx).len()
                );
                m.close(cx);
            });
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds(unread_selector).is_none(),
        "a reply stays read when the Mono closes"
    );
}

#[gpui::test]
fn sidebar_right_click_opens_mono_menu_outside_the_mono_and_edits_its_look(
    cx: &mut TestAppContext,
) {
    cx.update(input::bind);
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    // Leave the Mono, as when a session is open: only the rail remains.
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.active = None;
                cx.notify();
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let row = s
        .manager
        .read_with(visual, |m, _| m.rail_bounds[&s.id].get())
        .expect("the sidebar lists the Mono");
    let at = row.center();
    visual.simulate_event(gpui::MouseDownEvent {
        position: at,
        button: MouseButton::Right,
        modifiers: Default::default(),
        click_count: 1,
        first_mouse: false,
    });
    visual.simulate_event(gpui::MouseUpEvent {
        position: at,
        button: MouseButton::Right,
        modifiers: Default::default(),
        click_count: 1,
    });
    visual.run_until_parked();
    // MonoMenu opens beside the pointer with ten mascots and nine presets
    // plus the custom swatch, all in single rows.
    let first = visual
        .debug_bounds("mono-mascot-0")
        .expect("right-click opens the Mono's menu");
    let last = visual.debug_bounds("mono-mascot-9").unwrap();
    assert_eq!(first.top(), last.top());
    assert_eq!(first.size.height, px(32.));
    assert!(first.left() >= at.x);
    let custom = visual.debug_bounds("mono-custom-color").unwrap();
    assert_eq!(
        visual.debug_bounds("mono-color-0").unwrap().top(),
        custom.top()
    );
    let pink = visual.debug_bounds("mono-color-4").unwrap().center();
    visual.simulate_click(pink, Default::default());
    visual.run_until_parked();
    let color = s
        .manager
        .read_with(visual, |m, _| m.roster.get(&s.id).unwrap().color);
    assert_eq!(color, mono::COLORS_MONO[4]);
    // Enter in the name field commits and closes the menu.
    let name = visual.debug_bounds("mono-options-name").unwrap();
    visual.simulate_click(name.center(), Default::default());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    s.manager
        .read_with(visual, |m, _| assert!(m.menu.is_none()));
    visual.refresh().unwrap();
    visual
        .update_window(s.window.into(), |_, window, cx| {
            window.simulate_next_frame(cx);
            window.draw(cx).clear(cx);
        })
        .unwrap();
    assert!(visual.debug_bounds("mono-mascot-0").is_none());
}

#[gpui::test]
fn habits_and_documents_search_like_sessions(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let habit = |id: &str, name: &str| {
        let mut h = Habit::new(
            name.into(),
            "Do the work.".into(),
            habits::Schedule::default(),
            monocode_core::time::now_ms(),
        )
        .unwrap();
        h.id = id.into();
        h
    };
    let document = |id: &str, title: &str| mono::Document {
        id: id.into(),
        kind: "markdown".into(),
        title: title.into(),
        summary: String::new(),
        body: String::new(),
        created_at: 1,
        updated_at: 1,
    };
    s.window
        .update(cx, |_, _, cx| {
            s.manager.update(cx, |m, cx| {
                m.settings = None;
                m.overview_tab = OverviewTab::Habits;
                m.habits = vec![
                    habit("digest", "Morning digest"),
                    habit("report", "Weekly report"),
                ];
                let documents = &mut m.roster.get_mut(&s.id).unwrap().documents;
                documents.push(document("plan", "Launch plan"));
                documents.push(document("summary", "Release summary"));
                cx.notify();
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let search = |visual: &mut gpui::VisualTestContext, key: &str, text: &str| {
        s.manager.update(visual, |m, cx| {
            m.fields[key].update(cx, |f, cx| f.set_text(text.into(), cx));
        });
        visual.run_until_parked();
    };
    assert!(visual.debug_bounds("mono-habit-report").is_some());
    search(visual, "mono-habit-search", "DIGEST");
    assert!(visual.debug_bounds("mono-habit-digest").is_some());
    assert!(visual.debug_bounds("mono-habit-report").is_none());

    s.manager.update(visual, |m, cx| {
        m.select_overview(OverviewTab::Documents, cx)
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("mono-document-plan").is_some());
    search(visual, "mono-document-search", "release");
    assert!(visual.debug_bounds("mono-document-plan").is_none());
    assert!(visual.debug_bounds("mono-document-summary").is_some());
}

#[gpui::test]
fn the_principal_makes_bots_and_agents_write_to_each_other(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.manager.update(cx, |m, _| {
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.kind = Some("bot".into());
        mono.bot_role = Some("principal".into());
    });
    let made = call(
        &s,
        "make",
        "bots.create",
        json!({"role": "finance", "name": "Cash"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(made["bot"]["name"], "Cash");
    assert_eq!(made["bot"]["role"], "finance");
    assert!(
        made["bot"]["specialty"]
            .as_str()
            .unwrap()
            .starts_with("Money")
    );
    let refused = call(
        &s,
        "phone",
        "bots.create",
        json!({"role": "phone", "name": "P"}),
        cx,
    )
    .try_recv()
    .unwrap();
    assert!(refused.unwrap_err().contains("only the user creates one"));

    let list = call(&s, "list", "agents.list", json!({}), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    let agents = list["agents"].as_array().unwrap();
    assert!(agents.iter().any(|a| a["name"] == "Cash"));
    assert!(
        !agents.iter().any(|a| a["id"] == s.id.as_str()),
        "not itself"
    );

    let sent = call(
        &s,
        "ask",
        "agents.message",
        json!({"to": "cash", "message": "Sum October"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(sent["to"], "Cash");
    assert_eq!(sent["submitted"], true);
    s.manager.read_with(cx, |m, _| {
        let cash = m.roster.monos.iter().find(|m| m.name == "Cash").unwrap();
        let queued = &cash
            .outbox
            .last()
            .expect("the message waits in Cash's chat")
            .text;
        assert!(queued.contains("<agent_message from=") && queued.contains("Sum October"));
    });
    // Cash is answering: it may not pass the job on to a third agent.
    let cash = s.manager.read_with(cx, |m, _| {
        m.roster
            .monos
            .iter()
            .find(|m| m.name == "Cash")
            .unwrap()
            .id
            .clone()
    });
    s.manager.update(cx, |m, _| {
        assert!(
            m.agent_calls
                .messaging
                .is_answering(&cash, monocode_core::time::now_ms())
        );
    });

    let renamed = call(
        &s,
        "name",
        "name.update",
        json!({"name": "  Boss   Bot "}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(renamed["name"], "Boss Bot");
    s.manager.read_with(cx, |m, _| {
        assert_eq!(m.roster.get(&s.id).unwrap().name, "Boss Bot")
    });
}

#[gpui::test]
fn an_agents_reply_comes_back_whole_not_the_notification_clip(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.manager.update(cx, |m, _| {
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.kind = Some("bot".into());
        mono.bot_role = Some("principal".into());
    });
    call(
        &s,
        "make",
        "bots.create",
        json!({"role": "finance", "name": "Cash"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let cash = s.manager.update(cx, |m, cx| {
        let cash = m
            .roster
            .monos
            .iter()
            .find(|m| m.name == "Cash")
            .unwrap()
            .id
            .clone();
        let mono = m.roster.get_mut(&cash).unwrap();
        mono.kind = None;
        mono.bot_role = None;
        m.ensure_chat(&cash, cx);
        cash
    });
    cx.run_until_parked();
    s.manager.update(cx, |m, cx| {
        for id in [&s.id, &cash] {
            m.chats[id].update(cx, |chat, _| {
                chat.settings.usage_limit = Some(Default::default());
            });
        }
    });
    let request = format!("Sum October {}", "x".repeat(2500));
    call(
        &s,
        "ask",
        "agents.message",
        json!({"to": "cash", "message": request}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let (cash, sent_id, sent_text) = s.manager.read_with(cx, |m, _| {
        let cash = m.roster.monos.iter().find(|m| m.name == "Cash").unwrap();
        let last = cash.outbox.last().unwrap();
        (cash.id.clone(), last.id.clone(), last.text.clone())
    });
    // A reply a notification clip would mangle: a second paragraph plus a
    // fenced block, well past its 240-unit cap.
    let answer = "Hash: ca5bfeee\n\nThe details follow in a second paragraph that the \
        notification body would have dropped entirely, because it sits past the first \
        paragraph break and well past its two hundred and forty unit cap on purpose.\n\n\
        ```text\nfeat(groups): wire the native group dialog\n```";
    s.manager.update(cx, |m, cx| {
        m.inflight.insert(
            cash.clone(),
            monocode_core::mono::Message {
                id: sent_id.clone(),
                text: sent_text.clone(),
                sent_at: Some(1),
                internal: false,
                error: None,
                attachments: Vec::new(),
            },
        );
        assert!(
            !m.agent_answered(&cash, &sent_id, cx),
            "a missing request must not consume the route"
        );
        let retry_id = format!("{sent_id}-retry");
        m.agent_calls.retry_route(&sent_id, &retry_id);
        m.inflight.get_mut(&cash).unwrap().id = retry_id.clone();
        m.chats[&cash].update(cx, |view, cx| {
            view.transcript.update(cx, |t, cx| {
                let start = t.source_rows().len();
                t.patch(
                    start,
                    vec![
                        monocode_core::transcript::Row {
                            role: "user".into(),
                            text: sent_text[..1000].to_owned(),
                            first: true,
                            sent_at: Some(1),
                        },
                        monocode_core::transcript::Row {
                            role: "user".into(),
                            text: sent_text[1000..].to_owned(),
                            first: false,
                            sent_at: None,
                        },
                        monocode_core::transcript::Row {
                            role: "assistant".into(),
                            text: answer[..100].to_owned(),
                            first: true,
                            sent_at: Some(2),
                        },
                        monocode_core::transcript::Row {
                            role: "assistant".into(),
                            text: answer[100..].to_owned(),
                            first: false,
                            sent_at: None,
                        },
                    ],
                    cx,
                );
            });
        });
        assert!(m.agent_answered(&cash, &retry_id, cx));
        assert!(m.agent_answered(&cash, &retry_id, cx));
    });
    s.manager.read_with(cx, |m, _| {
        let reply = m
            .roster
            .get(&s.id)
            .unwrap()
            .outbox
            .last()
            .expect("the reply lands back in the asker's chat");
        assert!(reply.internal);
        assert!(reply.text.starts_with("<agent_reply from=\"Cash\">"));
        for part in [
            "Hash: ca5bfeee",
            "second paragraph",
            "two hundred and forty unit cap",
            "feat(groups): wire the native group dialog",
        ] {
            assert!(reply.text.contains(part), "reply is missing {part:?}");
        }
    });
    s.manager.update(cx, |m, _| {
        let entry = m
            .delegations()
            .iter()
            .find(|d| d.to == cash)
            .expect("the delegation was recorded");
        assert_eq!(entry.reply.as_deref(), Some(answer));
    });
    cx.run_until_parked();
    let saved = monocode_core::mono::delegations::load(&locked(&s.store).unwrap());
    let entry = saved.iter().find(|d| d.to == cash).unwrap();
    assert_eq!(entry.reply.as_deref(), Some(answer));
}

#[gpui::test]
fn a_stopped_agent_request_stays_retryable_without_a_false_reply(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.manager.update(cx, |m, _| {
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.kind = Some("bot".into());
        mono.bot_role = Some("principal".into());
    });
    call(
        &s,
        "make",
        "bots.create",
        json!({"role": "finance", "name": "Cash"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let cash = s.manager.update(cx, |m, cx| {
        let id = m
            .roster
            .monos
            .iter()
            .find(|m| m.name == "Cash")
            .unwrap()
            .id
            .clone();
        m.ensure_chat(&id, cx);
        id
    });
    cx.run_until_parked();
    s.manager.update(cx, |m, cx| {
        m.chats[&cash].update(cx, |chat, _| {
            chat.settings.usage_limit = Some(Default::default());
        });
    });
    call(
        &s,
        "ask",
        "agents.message",
        json!({"to": "cash", "message": "Find the invoice"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    s.manager.update(cx, |m, cx| {
        let request = m.roster.get(&cash).unwrap().outbox.last().unwrap().clone();
        m.inflight.insert(cash.clone(), request);
        m.chats[&cash].update(cx, |chat, cx| {
            chat.busy = true;
            chat.stop_if_busy(cx);
        });
    });
    cx.run_until_parked();
    s.manager.update(cx, |m, _| {
        let request = m.roster.get(&cash).unwrap().outbox.last().unwrap();
        assert!(request.error.as_deref().unwrap().contains("stopped"));
        assert!(
            !m.roster
                .get(&s.id)
                .unwrap()
                .outbox
                .iter()
                .any(|message| { message.text.starts_with("<agent_reply") })
        );
        let delegation = m.delegations().iter().find(|d| d.to == cash).unwrap();
        let id = delegation.id.clone();
        assert!(delegation.reply.is_none());
        assert!(m.agent_calls.pending_delegation(&id));
    });
}

#[gpui::test]
fn a_restarted_agent_routes_the_answer_to_the_original_delegation(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.manager.update(cx, |m, _| {
        let principal = m.roster.get_mut(&s.id).unwrap();
        principal.kind = Some("bot".into());
        principal.bot_role = Some("principal".into());
        principal.name = "Principal".into();
    });
    call(
        &s,
        "make",
        "bots.create",
        json!({"role": "finance", "name": "Cash"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    let cash = s.manager.update(cx, |m, cx| {
        let id = m
            .roster
            .monos
            .iter()
            .find(|m| m.name == "Cash")
            .unwrap()
            .id
            .clone();
        let mono = m.roster.get_mut(&id).unwrap();
        mono.kind = None;
        mono.bot_role = None;
        m.ensure_chat(&id, cx);
        id
    });
    cx.run_until_parked();
    s.manager.update(cx, |m, cx| {
        for id in [&s.id, &cash] {
            m.chats[id].update(cx, |chat, _| {
                chat.settings.usage_limit = Some(Default::default());
            });
        }
    });
    for (id, message) in [("first", "First request"), ("second", "Second request")] {
        call(
            &s,
            id,
            "agents.message",
            json!({"to": "cash", "message": message}),
            cx,
        )
        .try_recv()
        .unwrap()
        .unwrap();
    }
    s.manager.update(cx, |m, cx| {
        let first = m.roster.get(&cash).unwrap().outbox[0].clone();
        m.agent_calls.forget_routes();
        m.roster.get_mut(&s.id).unwrap().name = "Renamed Principal".into();
        m.inflight.insert(cash.clone(), first.clone());
        m.chats[&cash].update(cx, |view, cx| {
            view.transcript.update(cx, |transcript, cx| {
                let start = transcript.source_rows().len();
                transcript.patch(
                    start,
                    vec![
                        monocode_core::transcript::Row {
                            role: "user".into(),
                            text: first.text,
                            first: true,
                            sent_at: first.sent_at,
                        },
                        monocode_core::transcript::Row {
                            role: "assistant".into(),
                            text: "Answer to first request".into(),
                            first: true,
                            sent_at: None,
                        },
                    ],
                    cx,
                );
            });
        });
        assert!(m.agent_answered(&cash, &first.id, cx));
        let entries = m.delegations();
        assert_eq!(
            entries
                .iter()
                .find(|d| d.message == "First request")
                .unwrap()
                .reply
                .as_deref(),
            Some("Answer to first request")
        );
        assert!(
            entries
                .iter()
                .find(|d| d.message == "Second request")
                .unwrap()
                .reply
                .is_none()
        );
        assert!(
            m.roster
                .get(&s.id)
                .unwrap()
                .outbox
                .iter()
                .any(|message| { message.text.contains("Answer to first request") })
        );
    });
}

#[gpui::test]
fn a_bots_settings_show_its_specialty_and_save_its_tools(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    s.manager.update(cx, |m, cx| {
        let mono = m.roster.get_mut(&s.id).unwrap();
        mono.kind = Some("bot".into());
        mono.bot_role = Some("principal".into());
        cx.notify();
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    assert!(visual.debug_bounds("mono-orchestrator-badge").is_some());
    assert!(visual.debug_bounds("mono-specialty-field").is_some());
    s.manager.update(visual, |m, cx| {
        m.save_bot_tools(&s.id, |t| t.access.retain(|a| a != "web"), cx);
        m.bot_field_changed("mono-specialty", "  Runs   the team  ", cx);
    });
    let mono = s
        .manager
        .read_with(visual, |m, _| m.roster.get(&s.id).unwrap().clone());
    let access = mono.bot_tools.unwrap().access.unwrap();
    assert!(!access.iter().any(|a| a == "web"));
    assert!(access.iter().any(|a| a == "read"));
    assert_eq!(mono.specialty.as_deref(), Some("Runs the team"));
}

struct CallTestVoice {
    spoken: mpsc::Sender<String>,
}

impl monobot_voice::queue::Backend for CallTestVoice {
    fn synthesize(&self, text: &str, _: &str, _: i32) -> Result<Vec<u8>, String> {
        Ok(text.as_bytes().to_vec())
    }

    fn play(&self, audio: &[u8], _: Arc<std::sync::atomic::AtomicBool>) -> Result<(), String> {
        self.spoken
            .send(String::from_utf8_lossy(audio).into_owned())
            .map_err(|error| error.to_string())
    }
}

#[gpui::test]
fn group_call_streams_a_member_and_saves_the_transcript(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::{Group, native::Mark};
    use monocode_core::transcript::Row;
    let s = setup(cx);
    let fake_mic = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(fake_mic)));
    let (member, key, before) = s.manager.update(cx, |m, cx| {
        let member = m
            .roster
            .create_bot(&m.root, "finance", false)
            .unwrap()
            .id
            .clone();
        m.roster.get_mut(&member).unwrap().name = "Cash".into();
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.name = "Team".into();
        group.group = Some(Group {
            members: vec![member.clone()],
            ..Group::default()
        });
        let key = (s.id.clone(), member.clone());
        assert!(m.ensure_seat(&key, cx));
        let before = m.chats[&s.id].read(cx).mono_rows(cx).len();
        (member, key, before)
    });
    cx.run_until_parked();
    let (spoken, heard) = mpsc::channel();
    let (seat, request) = s.manager.update(cx, |m, cx| {
        let seat = m.groups.seats.get_mut(&key).unwrap().view.take().unwrap();
        let group = m.roster.get(&s.id).unwrap().clone();
        assert!(panels::overview_tabs(&group).contains(&OverviewTab::Call));
        m.overview_tab = OverviewTab::Call;
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_say("Cash, what happened?", cx);
        let (target, request) = m.call_test_current_request().unwrap();
        assert_eq!(target, member);
        seat.update(cx, |seat, cx| {
            seat.input
                .update(cx, |input, cx| input.set_text("hold".into(), cx));
        });
        m.groups.seats.get_mut(&key).unwrap().view = Some(seat.clone());
        (seat, request)
    });
    let answer_index = s.manager.update(cx, |m, cx| {
        let index = seat.update(cx, |seat, cx| {
            seat.busy = true;
            seat.transcript.update(cx, |transcript, cx| {
                let start = transcript.source_rows().len();
                transcript.patch(
                    start,
                    vec![
                        Mark::GroupRequest(request.clone()).row(Some(1)),
                        Row {
                            role: "user".into(),
                            text: "call prompt".into(),
                            first: true,
                            sent_at: Some(1),
                        },
                        Row {
                            role: "assistant".into(),
                            text: "First sentence. ".into(),
                            first: true,
                            sent_at: None,
                        },
                    ],
                    cx,
                );
                start + 2
            })
        });
        m.call_progress(cx);
        index
    });
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(5)).unwrap(),
        "First sentence."
    );
    s.manager.update(cx, |m, cx| {
        seat.update(cx, |seat, cx| {
            seat.busy = false;
            seat.transcript.update(cx, |transcript, cx| {
                transcript.patch(
                    answer_index,
                    vec![Row {
                        role: "assistant".into(),
                        text: "First sentence. Second sentence.".into(),
                        first: true,
                        sent_at: None,
                    }],
                    cx,
                );
            });
        });
        m.call_progress(cx);
        let lines = m.call_test_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[1].text, "First sentence. Second sentence.");
        assert!(m.roster.get(&s.id).unwrap().outbox.is_empty());
        assert_eq!(m.chats[&s.id].read(cx).mono_rows(cx).len(), before);
    });
    assert_eq!(
        heard.recv_timeout(Duration::from_secs(5)).unwrap(),
        "Second sentence."
    );
    s.manager.update(cx, |m, cx| {
        m.select_overview(OverviewTab::Members, cx);
        let documents = &m.roster.get(&s.id).unwrap().documents;
        assert_eq!(documents.len(), 1);
        assert!(
            documents[0]
                .body
                .contains("**Cash:** First sentence. Second sentence.")
        );
    });
    cx.run_until_parked();
    let roster = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert!(
        roster.get(&s.id).unwrap().documents[0]
            .body
            .contains("**You:** Cash, what happened?")
    );
}

#[gpui::test]
fn interrupting_a_call_cancels_only_its_queued_request(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    let mic = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(mic)));
    let (member, key) = s.manager.update(cx, |m, cx| {
        let member = m
            .roster
            .create_bot(&m.root, "finance", false)
            .unwrap()
            .id
            .clone();
        m.roster.get_mut(&member).unwrap().name = "Cash".into();
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group {
            members: vec![member.clone()],
            ..Group::default()
        });
        let key = (s.id.clone(), member.clone());
        assert!(m.ensure_seat(&key, cx));
        (member, key)
    });
    cx.run_until_parked();
    let (spoken, _heard) = mpsc::channel();
    s.manager.update(cx, |m, cx| {
        let seat = m.groups.seats.get_mut(&key).unwrap().view.take().unwrap();
        let group = m.roster.get(&s.id).unwrap().clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_say("Cash, respond", cx);
        let (_, request) = m.call_test_current_request().unwrap();
        seat.update(cx, |seat, cx| {
            seat.input
                .update(cx, |input, cx| input.set_text("hold".into(), cx));
        });
        m.groups.seats.get_mut(&key).unwrap().view = Some(seat);
        assert!(m.call_test_queued(&s.id, &member, &request));
        m.send_to_member(
            &s.id,
            &member,
            "Other request".into(),
            "other".into(),
            Vec::new(),
            false,
            cx,
        );
        m.call_test_interrupt(cx);
        assert!(!m.call_test_queued(&s.id, &member, &request));
        assert!(m.call_test_queued(&s.id, &member, "other"));
        assert!(m.call_test_current_request().is_none());
        assert_eq!(
            m.call_test_lines()[1].status,
            monocode_core::mono::groups::call::LineStatus::Stopped
        );
        m.call_hang_up(cx);
        assert!(m.roster.get(&s.id).unwrap().documents.is_empty());
    });
    cx.run_until_parked();
}

#[gpui::test]
fn group_call_sends_dictation_after_the_user_pauses(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    let mic = crate::dictation::tests::Fake::installed("Cash hello");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(mic.clone())));
    let (member, key) = s.manager.update(cx, |m, cx| {
        let member = m
            .roster
            .create_bot(&m.root, "finance", false)
            .unwrap()
            .id
            .clone();
        m.roster.get_mut(&member).unwrap().name = "Cash".into();
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group {
            members: vec![member.clone()],
            ..Group::default()
        });
        let key = (s.id.clone(), member.clone());
        assert!(m.ensure_seat(&key, cx));
        (member, key)
    });
    cx.run_until_parked();
    let (spoken, _heard) = mpsc::channel();
    let seat = s.manager.update(cx, |m, cx| {
        let seat = m.groups.seats.get_mut(&key).unwrap().view.take().unwrap();
        let group = m.roster.get(&s.id).unwrap().clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_input().update(cx, |input, cx| {
            input.set_text("Unsent draft".into(), cx);
        });
        m.call_test_dictated().update(cx, |input, cx| {
            input.set_text("Stale dictated text".into(), cx);
        });
        m.call_test_interrupt(cx);
        seat
    });
    cx.run_until_parked();
    mic.hear(monobot_dictation::Event::Speech(true));
    mic.hear(monobot_dictation::Event::Partial("Cash hello".into()));
    mic.hear(monobot_dictation::Event::Speech(false));
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_millis(600));
    cx.run_until_parked();
    s.manager.update(cx, |m, cx| {
        assert_eq!(m.call_test_lines()[0].text, "Cash hello");
        assert_eq!(m.call_test_current_request().unwrap().0, member);
        assert_eq!(
            m.call_test_input().read(cx).content.as_ref(),
            "Unsent draft"
        );
        seat.update(cx, |seat, cx| {
            seat.input
                .update(cx, |input, cx| input.set_text("hold".into(), cx));
        });
        m.groups.seats.get_mut(&key).unwrap().view = Some(seat);
        m.call_hang_up(cx);
    });
    cx.run_until_parked();
    assert!(mic.calls().iter().any(|call| call == "stop 7"));
}

#[gpui::test]
fn group_call_skips_a_member_with_an_unsent_draft(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::{Group, call::LineStatus};
    let s = setup(cx);
    let mic = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(mic)));
    let (member, key) = s.manager.update(cx, |m, cx| {
        let member = m
            .roster
            .create_bot(&m.root, "finance", false)
            .unwrap()
            .id
            .clone();
        m.roster.get_mut(&member).unwrap().name = "Cash".into();
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group {
            members: vec![member.clone()],
            ..Group::default()
        });
        let key = (s.id.clone(), member.clone());
        assert!(m.ensure_seat(&key, cx));
        (member, key)
    });
    cx.run_until_parked();
    let (spoken, _heard) = mpsc::channel();
    s.manager.update(cx, |m, cx| {
        let seat = m.groups.seats[&key].view.clone().unwrap();
        seat.update(cx, |seat, cx| {
            seat.input
                .update(cx, |input, cx| input.set_text("draft".into(), cx));
        });
        let group = m.roster.get(&s.id).unwrap().clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_say("Cash, please answer", cx);
        assert!(m.call_test_current_request().is_none());
        let lines = m.call_test_lines();
        assert_eq!(lines[1].who, member);
        assert_eq!(lines[1].status, LineStatus::Busy);
        m.call_hang_up(cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn group_call_recovers_mic_errors_and_keeps_muted_installation_quiet(cx: &mut TestAppContext) {
    use monocode_core::{dictation::Phase, mono::groups::Group};
    let s = setup(cx);
    let fake = crate::dictation::tests::Fake::installed("");
    *fake.start_error.lock().unwrap() = Some("Microphone unavailable".into());
    cx.update(|cx| cx.set_global(crate::dictation::Engines(fake.clone())));
    let (spoken, _heard) = mpsc::channel();
    let mic = s.manager.update(cx, |m, cx| {
        let member = m
            .roster
            .create_bot(&m.root, "finance", false)
            .unwrap()
            .id
            .clone();
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group {
            members: vec![member],
            ..Group::default()
        });
        let group = group.clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_mic()
    });
    cx.run_until_parked();
    assert_eq!(
        mic.read_with(cx, |mic, _| mic.error().map(str::to_owned))
            .as_deref(),
        Some("Microphone unavailable")
    );
    *fake.start_error.lock().unwrap() = None;
    mic.update(cx, |mic, cx| mic.dismiss(cx));
    cx.run_until_parked();
    assert_eq!(mic.read_with(cx, |mic, _| mic.phase()), Phase::Listening);

    s.manager.update(cx, |m, cx| m.call_test_mute(cx));
    cx.run_until_parked();
    *fake.installed.lock().unwrap() = false;
    mic.update(cx, |mic, cx| mic.install(cx));
    cx.run_until_parked();
    assert_eq!(mic.read_with(cx, |mic, _| mic.phase()), Phase::Idle);
    assert!(
        fake.calls()
            .iter()
            .filter(|call| *call == "cancel 7")
            .count()
            >= 2
    );
    s.manager.update(cx, |m, cx| m.call_hang_up(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn group_call_tab_is_visible_and_closing_the_panel_hangs_up(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let mic = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(mic.clone())));
    s.window
        .update(cx, |shell, _, cx| {
            shell.workspace.right_panel_width = 400.;
            s.manager.update(cx, |m, cx| {
                let member = m
                    .roster
                    .create_bot(&m.root, "finance", false)
                    .unwrap()
                    .id
                    .clone();
                m.roster.get_mut(&member).unwrap().name = "Cash".into();
                let group = m.roster.get_mut(&s.id).unwrap();
                group.kind = Some("group".into());
                group.group = Some(Group {
                    members: vec![member],
                    ..Group::default()
                });
                m.settings = None;
                m.select_overview(OverviewTab::Members, cx);
            });
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    let tab = visual.debug_bounds("mono-overview-Call").unwrap();
    visual.simulate_click(tab.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("group-call-content").is_some());
    assert!(visual.debug_bounds("group-call-start").is_some());
    let (spoken, _heard) = mpsc::channel();
    s.manager.update(visual, |m, cx| {
        let group = m.roster.get(&s.id).unwrap().clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
    });
    visual.run_until_parked();
    assert!(visual.debug_bounds("group-call-hang-up").is_some());
    let toggle = visual.debug_bounds("toggle-right-panel").unwrap();
    visual.simulate_click(toggle.center(), Default::default());
    visual.run_until_parked();
    assert!(s.manager.read_with(visual, |m, _| m.call.is_none()));
    assert!(mic.calls().iter().any(|call| call == "cancel 7"));
}

#[gpui::test]
fn old_voice_setup_cannot_change_a_new_call(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    let fake = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(fake)));
    s.manager.update(cx, |m, cx| {
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group::default());
        let group = group.clone();
        let old = m.call_test_pending_voice(&group, cx);
        m.call_hang_up(cx);
        let current = m.call_test_pending_voice(&group, cx);
        assert_ne!(old, current);
        m.call_test_voice_failure(&s.id, old, cx);
        assert!(m.call_test_voice_error().is_none());
        m.call_test_voice_failure(&s.id, current, cx);
        assert_eq!(m.call_test_voice_error().as_deref(), Some("voice failure"));
        m.call_hang_up(cx);
    });
    cx.run_until_parked();
}

#[gpui::test]
fn old_silence_timer_cannot_finish_a_new_calls_turn(cx: &mut TestAppContext) {
    use monocode_core::{dictation::Phase, mono::groups::Group};
    let s = setup(cx);
    let fake = crate::dictation::tests::Fake::installed("");
    cx.update(|cx| cx.set_global(crate::dictation::Engines(fake.clone())));
    let (spoken, _heard) = mpsc::channel();
    s.manager.update(cx, |m, cx| {
        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.group = Some(Group::default());
        let group = group.clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
    });
    cx.run_until_parked();
    let say_partial = |fake: &crate::dictation::tests::Fake, cx: &mut TestAppContext| {
        fake.hear(monobot_dictation::Event::Speech(true));
        fake.hear(monobot_dictation::Event::Partial("hola".into()));
        fake.hear(monobot_dictation::Event::Speech(false));
        cx.run_until_parked();
    };
    say_partial(&fake, cx);
    let old = s.manager.update(cx, |m, cx| {
        let epoch = m.call_test_pause_epoch();
        m.call_hang_up(cx);
        epoch
    });
    cx.run_until_parked();
    let (spoken, _heard) = mpsc::channel();
    let mic = s.manager.update(cx, |m, cx| {
        let group = m.roster.get(&s.id).unwrap().clone();
        m.call_test_start(&group, CallTestVoice { spoken }, cx);
        m.call_test_mic()
    });
    cx.run_until_parked();
    say_partial(&fake, cx);
    s.manager.update(cx, |m, cx| {
        let current = m.call_test_pause_epoch();
        assert_ne!(old, current);
        m.call_test_end_silence(&s.id, old, cx);
        assert_eq!(mic.read(cx).phase(), Phase::Listening);
        m.call_test_end_silence(&s.id, current, cx);
        assert_eq!(mic.read(cx).phase(), Phase::Finishing);
    });
    cx.run_until_parked();
    assert!(fake.calls().iter().any(|call| call == "stop 7"));
    s.manager.update(cx, |m, cx| m.call_hang_up(cx));
    cx.run_until_parked();
}

#[gpui::test]
fn a_group_asks_its_members_in_their_seats_and_posts_their_answers(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::{Access, Block, Group, native};
    let s = setup(cx);
    // Members whose chats never reach a provider: their seats follow them
    // into a folder that does not exist, so each turn stops on an error
    // before anything launches, and that error is the answer they post.
    let fixture = || {
        locked(&s.store)
            .unwrap()
            .create("codex", "/missing/monobot-group-member")
            .unwrap()
            .id
    };
    let (cash_chat, docs_chat) = (fixture(), fixture());
    let handle: gpui::AnyWindowHandle = s.window.into();
    let (cash, docs) = handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                let root = m.root.clone();
                let mut bot = |role: &str, name: &str, session: i64| {
                    let id = m.roster.create_bot(&root, role, false).unwrap().id.clone();
                    let bot = m.roster.get_mut(&id).unwrap();
                    bot.name = name.into();
                    bot.session_id = Some(session);
                    id
                };
                let cash = bot("finance", "Cash", cash_chat);
                let docs = bot("documents", "Docs", docs_chat);
                let group = m.roster.get_mut(&s.id).unwrap();
                group.kind = Some("group".into());
                group.name = "Team".into();
                group.group = Some(Group {
                    members: vec![cash.clone(), docs.clone()],
                    goal: Some("Close October".into()),
                    ..Group::default()
                });
                // The lead's own turns stay queued: no provider starts.
                m.chats[&s.id].update(cx, |chat, _| {
                    chat.settings.usage_limit = Some(Default::default())
                });
                m.configure_chat(&s.id, cx);
                (cash, docs)
            })
        })
        .unwrap();
    cx.run_until_parked();
    let members = call(&s, "members", "group.members", json!({}), cx)
        .try_recv()
        .unwrap()
        .unwrap();
    let names: Vec<_> = members["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Cash", "Docs"]);

    let chat = cx.update(|cx| s.manager.read(cx).chats[&s.id].clone());
    let say = |text: &str, cx: &mut TestAppContext| {
        chat.update(cx, |_, cx| {
            cx.emit(crate::conversation::MonoSubmitted {
                text: text.into(),
                attachments: Vec::new(),
            })
        });
        cx.run_until_parked();
    };
    let group_blocks =
        |cx: &mut TestAppContext| cx.update(|cx| native::blocks(chat.read(cx).mono_rows(cx)));
    let seat_of = |member: &str, cx: &mut TestAppContext| {
        cx.update(|cx| {
            s.manager
                .read(cx)
                .roster
                .get(&s.id)
                .unwrap()
                .seats
                .get(member)
                .copied()
        })
    };
    let lead_outbox = |cx: &mut TestAppContext| {
        cx.update(|cx| s.manager.read(cx).roster.get(&s.id).unwrap().outbox.clone())
    };
    let posts = |blocks: &[Block], member: &str, kind: &str| {
        blocks
            .iter()
            .filter(|b| {
                b.group_member
                    .as_ref()
                    .is_some_and(|p| p.mono_id == member && p.request.contains(kind))
            })
            .count()
    };

    // @Name, anywhere in the message, goes to that member alone.
    say("oye @Cash, ¿cuánto gasté?", cx);
    let blocks = group_blocks(cx);
    let asked = blocks
        .iter()
        .find(|b| b.is_user() && b.text == "oye @Cash, ¿cuánto gasté?")
        .expect("the user's message shows as written");
    assert!(
        asked
            .app_request_id
            .as_deref()
            .is_some_and(|r| r.starts_with("group-direct-"))
    );
    let seat = seat_of(&cash, cx).expect("Cash answers from its own seat");
    assert_ne!(seat, cash_chat, "not from its own chat");
    assert!(seat_of(&docs, cx).is_none(), "Docs was not asked");
    let seat_rows = locked(&s.store).unwrap().load(seat).unwrap().rows;
    let prompt = native::blocks(&seat_rows)
        .into_iter()
        .find(|b| b.is_user())
        .unwrap();
    assert!(prompt.app_request_id.unwrap().contains("-direct-"));
    assert!(prompt.text.contains("<group_message group=\"Team\">"));
    assert!(prompt.text.contains("The group is for: Close October"));
    assert!(prompt.text.contains("oye Cash, ¿cuánto gasté?"));
    assert!(!prompt.text.contains("@Cash"));
    // Its finished answer posts in the group's chat under its name: here,
    // the error its turn stopped on.
    let posted = group_blocks(cx);
    assert_eq!(posts(&posted, &cash, "-direct-"), 1);
    let (post, text) = posted
        .iter()
        .find_map(|b| b.group_member.as_ref().map(|p| (p.clone(), b.text.clone())))
        .unwrap();
    assert_eq!(post.name, "Cash");
    assert!(text.starts_with("Couldn't answer:"), "{text}");
    assert!(lead_outbox(cx).is_empty(), "the lead was not asked");

    // A question for the team: every member answers in its seat, then the
    // lead reads them all and has the last word.
    say("¿Qué opinan del plan de cierre?", cx);
    assert_eq!(seat_of(&cash, cx), Some(seat), "a member keeps its seat");
    assert!(seat_of(&docs, cx).is_some());
    let posted = group_blocks(cx);
    assert_eq!(posts(&posted, &cash, "-round-"), 1);
    assert_eq!(posts(&posted, &docs, "-round-"), 1);
    let outbox = lead_outbox(cx);
    let last_word = outbox
        .iter()
        .find(|m| m.text.contains("<group_round>"))
        .expect("the lead has the last word");
    assert!(last_word.internal);
    assert!(
        last_word
            .text
            .contains("The user wrote to the group: \"¿Qué opinan del plan de cierre?\"")
    );
    assert!(
        last_word
            .text
            .contains("### Cash\n(no answer: it stopped on an error")
    );
    assert!(last_word.text.contains("### Docs"));

    // The lead brings a member in; its answer posts and comes back to it.
    let asked = call(
        &s,
        "ask-docs",
        "group.ask",
        json!({"member": "docs", "message": "Find the October invoices"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(asked["member"], "Docs");
    assert_eq!(asked["notifyOnComplete"], true);
    cx.run_until_parked();
    let posted = group_blocks(cx);
    assert!(posted.iter().any(|b| {
        b.group_member
            .as_ref()
            .is_some_and(|p| p.request.ends_with("-ask-docs"))
    }));
    assert!(lead_outbox(cx).iter().any(|m| {
        m.internal
            && m.text.starts_with("MonoBOT completion notification")
            && m.text.contains("Find the October invoices")
    }));
    let refused = call(
        &s,
        "ask-nobody",
        "group.ask",
        json!({"member": "ivy", "message": "x"}),
        cx,
    )
    .try_recv()
    .unwrap();
    assert_eq!(
        refused.unwrap_err(),
        "No member by that name or id; run group.members"
    );

    // A path outside the group's folders: the user is asked in the chat.
    let access = call(
        &s,
        "access",
        "group.access",
        json!({"target": "D:\\Survey", "reason": "the survey data"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(access["access"], "asked");
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("group-access-prompt").is_some());
    assert!(visual.debug_bounds("mono-signature-name-Cash").is_some());
    let request = visual.update(|_, cx| {
        s.manager
            .read(cx)
            .roster
            .get(&s.id)
            .unwrap()
            .group
            .as_ref()
            .unwrap()
            .requests[0]
            .id
            .clone()
    });
    chat.update(visual, |_, cx| {
        cx.emit(crate::conversation::GroupAccessAnswer {
            request,
            access: Access::Read,
        })
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("group-access-prompt").is_none());
    let group = visual.update(|_, cx| {
        s.manager
            .read(cx)
            .roster
            .get(&s.id)
            .unwrap()
            .group
            .clone()
            .unwrap()
    });
    assert!(group.requests.is_empty());
    assert!(
        group
            .folders
            .iter()
            .any(|f| f.path == "D:\\Survey" && !f.edit)
    );
    // The answer goes to the chat, so the group carries on.
    let blocks = visual.update(|_, cx| native::blocks(chat.read(cx).mono_rows(cx)));
    assert!(blocks.iter().any(|b| {
        b.is_user()
            && b.text
                .starts_with("Yes, read only: the members may read D:\\Survey")
    }));
    // What the group holds reads back from the saved roster.
    let saved = mono::Roster::load(&locked(&s.store).unwrap()).unwrap();
    let saved = saved.get(&s.id).unwrap();
    assert_eq!(
        saved.group.as_ref().unwrap().members,
        [cash.clone(), docs.clone()]
    );
    assert_eq!(saved.seats.get(&cash), Some(&seat));
}

#[gpui::test]
fn a_file_attached_to_a_group_message_goes_to_the_member_it_names(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::{Group, group::group_access_for, native};
    let s = setup(cx);
    let fixture = || {
        locked(&s.store)
            .unwrap()
            .create("codex", "/missing/monobot-group-member")
            .unwrap()
            .id
    };
    let docs_chat = fixture();
    let handle: gpui::AnyWindowHandle = s.window.into();
    let docs = handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                let root = m.root.clone();
                let docs = m
                    .roster
                    .create_bot(&root, "documents", false)
                    .unwrap()
                    .id
                    .clone();
                let bot = m.roster.get_mut(&docs).unwrap();
                bot.name = "Docs".into();
                bot.session_id = Some(docs_chat);
                let group = m.roster.get_mut(&s.id).unwrap();
                group.kind = Some("group".into());
                group.name = "Team".into();
                group.group = Some(Group {
                    members: vec![docs.clone()],
                    ..Group::default()
                });
                // The lead's own turns stay queued: no provider starts.
                m.chats[&s.id].update(cx, |chat, _| {
                    chat.settings.usage_limit = Some(Default::default())
                });
                m.configure_chat(&s.id, cx);
                docs
            })
        })
        .unwrap();
    cx.run_until_parked();
    let file = std::env::temp_dir().join("monobot-group-attachment.pdf");
    std::fs::write(&file, b"%PDF-1.4\n").unwrap();
    let chat = cx.update(|cx| s.manager.read(cx).chats[&s.id].clone());
    chat.update(cx, |_, cx| {
        cx.emit(crate::conversation::MonoSubmitted {
            text: "@Docs ¿de qué trata esto?".into(),
            attachments: vec![file.clone()],
        })
    });
    cx.run_until_parked();

    // The message went to Docs, not to the lead's outbox.
    let seat = cx.update(|cx| {
        s.manager
            .read(cx)
            .roster
            .get(&s.id)
            .unwrap()
            .seats
            .get(&docs)
            .copied()
    });
    assert!(seat.is_some(), "Docs was asked in its own seat");
    assert!(
        cx.update(|cx| s.manager.read(cx).roster.get(&s.id).unwrap().outbox.clone())
            .is_empty(),
        "the lead did not take the message"
    );
    let rows = cx.update(|cx| chat.read(cx).mono_rows(cx).to_vec());
    let blocks = native::blocks(&rows);
    assert!(
        blocks.iter().any(|b| b.is_user()
            && b.text == "@Docs ¿de qué trata esto?"
            && b.app_request_id
                .as_deref()
                .is_some_and(|r| r.starts_with("group-direct-"))),
        "the group's chat shows the message as written"
    );
    let manifest = rows
        .iter()
        .find(|row| row.role == monocode_core::transcript::attachments::ROLE)
        .expect("the chat keeps the file's chip");
    assert_eq!(
        monocode_core::transcript::attachments::paths(manifest).unwrap(),
        vec![file.clone()]
    );
    // Docs may read what the user handed it, without asking for access.
    let group = cx.update(|cx| {
        s.manager
            .read(cx)
            .roster
            .get(&s.id)
            .unwrap()
            .group
            .clone()
            .unwrap()
    });
    assert_eq!(
        group_access_for(&group, &file.to_string_lossy()),
        Some(monocode_core::mono::groups::Access::Read)
    );
    // A member with an ask in hand reads as working, so the members and
    // activity panels can say so while its seat answers.
    cx.update(|cx| {
        s.manager.update(cx, |m, cx| {
            assert_eq!(m.member_state(&s.id, &docs, cx), "idle");
            m.send_to_member(
                &s.id,
                &docs,
                "ping".into(),
                "req-ping".into(),
                Vec::new(),
                false,
                cx,
            );
            assert_eq!(m.member_state(&s.id, &docs, cx), "working");
            let group = m.current().cloned().unwrap();
            assert_eq!(m.group_working(&group, cx).len(), 1);
        })
    });
    // Tapping the file in the chat opens it beside the group's documents.
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    // The chat settles its scroll over a few frames after its rows lay out.
    let mut chip = None;
    for _ in 0..8 {
        visual.refresh().unwrap();
        visual.run_until_parked();
        let next = visual.debug_bounds("transcript-file-0");
        if next.is_some() && next == chip {
            break;
        }
        chip = next;
    }
    let chip = chip.expect("the attached file shows as a chip in the chat");
    visual.simulate_click(chip.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        visual.update(|_, cx| s.manager.read(cx).route.clone()),
        Some(Route::Paper(file.to_string_lossy().into_owned())),
        "the PDF opens in the panel, not in a code pane"
    );
    let _ = std::fs::remove_file(&file);
}

#[gpui::test]
fn the_group_speaks_with_the_principals_identity_in_its_transcript(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                let root = m.root.clone();
                let id = m
                    .roster
                    .create_bot(&root, "principal", false)
                    .unwrap()
                    .id
                    .clone();
                let bot = m.roster.get_mut(&id).unwrap();
                bot.name = "IVY".into();
                bot.mascot = "principal".into();
                bot.color = 0x5a9eff;

                let group = m.roster.get_mut(&s.id).unwrap();
                group.kind = Some("group".into());
                group.name = "Testing Group".into();
                group.mascot = "tree".into();
                group.group = Some(Group {
                    members: vec![id.clone()],
                    ..Group::default()
                });
                m.configure_chat(&s.id, cx);
            })
        })
        .unwrap();
    cx.run_until_parked();

    let (speaker_mascot, speaker_color, speaker_name) = cx.update(|cx| {
        let m = s.manager.read(cx);
        let group = m.roster.get(&s.id).unwrap();
        m.mono_speaker(group)
    });
    assert_eq!(speaker_name, "IVY");
    assert_eq!(speaker_mascot, "principal");
    assert_eq!(speaker_color, 0x5a9eff);

    let chat = cx.update(|cx| s.manager.read(cx).chats[&s.id].clone());
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    chat.update(visual, |chat, cx| {
        let mut transcript = monocode_core::transcript::Transcript::default();
        transcript.begin("assistant");
        transcript.append("Hola desde el líder");
        chat.mono_append(transcript.rows, cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("mono-signature-name-IVY").is_some());
    assert!(
        visual
            .debug_bounds("mono-signature-name-Testing Group")
            .is_none()
    );
}

#[gpui::test]
fn group_avatar_stack_renders_lead_first_and_counts_rest(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    let (shown, more) = s.manager.update(cx, |m, cx| {
        m.settings = None;
        m.close(cx);
        let root = m.root.clone();
        let p_id = m
            .roster
            .create_bot(&root, "principal", false)
            .unwrap()
            .id
            .clone();
        m.roster.get_mut(&p_id).unwrap().mascot = "principal".into();
        let b1_id = m
            .roster
            .create_bot(&root, "assistant", false)
            .unwrap()
            .id
            .clone();
        let b2_id = m
            .roster
            .create_bot(&root, "assistant", false)
            .unwrap()
            .id
            .clone();
        let b3_id = m
            .roster
            .create_bot(&root, "assistant", false)
            .unwrap()
            .id
            .clone();

        let group = m.roster.get_mut(&s.id).unwrap();
        group.kind = Some("group".into());
        group.name = "Test Stack Group".into();
        group.group = Some(Group {
            members: vec![b1_id, p_id.clone(), b2_id, b3_id],
            ..Group::default()
        });

        let group_mono = m.roster.get(&s.id).unwrap().clone();
        m.group_stack_members(&group_mono)
    });

    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].mascot, "principal");
    assert_eq!(more, 2);

    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    s.window
        .update(visual, |shell, window, cx| {
            shell.workspace.classic_layout = true;
            shell.workspace.right_panel_open = false;
            s.manager.update(cx, |m, cx| m.open(&s.id, window, cx));
            cx.notify();
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();

    let stack = visual
        .debug_bounds("group-avatar-stack")
        .expect("group avatar stack rendered");
    let name = visual
        .debug_bounds("original-mono-name")
        .expect("group title rendered");
    assert_eq!(stack.size.width, px(42.0));
    assert!(
        name.origin.x >= stack.origin.x + stack.size.width,
        "name should start after avatar stack: name.x={}, stack.end={}",
        name.origin.x,
        stack.origin.x + stack.size.width
    );
}

#[gpui::test]
fn the_principal_reads_its_bots_news_and_who_it_asked(cx: &mut TestAppContext) {
    use monocode_core::mono::{
        delegations::Delegation,
        news::{Feed, Post},
    };
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let bot = s.manager.update(cx, |m, cx| {
        let principal = m.roster.get_mut(&s.id).unwrap();
        principal.kind = Some("bot".into());
        principal.bot_role = Some("principal".into());
        let root = m.root.clone();
        let bot = m
            .roster
            .create_bot(&root, "finance", false)
            .unwrap()
            .id
            .clone();
        let mut feed = Feed::default();
        feed.add(
            Post {
                mono_id: bot.clone(),
                bot_name: "Cash".into(),
                source: "Weekly budget".into(),
                text: "Spending is **under** budget.".into(),
                ..Default::default()
            },
            monocode_core::time::now_ms(),
        );
        *m.news_feed() = feed;
        m.delegations().push(Delegation {
            id: "d1".into(),
            from: s.id.clone(),
            to: bot.clone(),
            message: "Check the invoices".into(),
            at: monocode_core::time::now_ms(),
            reply: None,
        });
        m.select_overview(OverviewTab::News, cx);
        bot
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    assert!(visual.debug_bounds("mono-overview-News").is_some());
    assert!(visual.debug_bounds("mono-overview-Bots").is_some());
    assert!(visual.debug_bounds("mono-overview-Sessions").is_none());
    assert!(visual.debug_bounds("bot-news-item").is_some());
    assert!(
        !s.manager.update(visual, |m, _| m.has_unseen_news()),
        "reading the feed marks it seen"
    );
    s.manager
        .update(visual, |m, cx| m.select_overview(OverviewTab::Bots, cx));
    visual.run_until_parked();
    assert!(visual.debug_bounds("bot-delegations").is_some());
    let _ = bot;
}

#[gpui::test]
fn bots_open_on_their_own_overview_tabs(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let (principal, bot) = s.manager.update(cx, |m, _| {
        let root = m.root.clone();
        let principal = m
            .roster
            .create_bot(&root, "principal", false)
            .unwrap()
            .id
            .clone();
        let bot = m
            .roster
            .create_bot(&root, "finance", false)
            .unwrap()
            .id
            .clone();
        (principal, bot)
    });
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    let open = |visual: &mut gpui::VisualTestContext, id: &str| {
        s.window
            .update(visual, |_, w, cx| {
                s.manager.update(cx, |m, cx| m.open(id, w, cx))
            })
            .unwrap();
        visual.run_until_parked();
    };
    open(visual, &principal);
    s.manager.update(visual, |m, _| {
        let mono = m.roster.get(&principal).unwrap();
        assert!(m.overview_tab == OverviewTab::News);
        assert!(m.shown_overview_tab(mono) == OverviewTab::News);
    });
    open(visual, &bot);
    s.manager.update(visual, |m, _| {
        let mono = m.roster.get(&bot).unwrap();
        assert!(m.overview_tab == OverviewTab::Activity);
        assert!(m.overview_previous == OverviewTab::Habits);
        assert!(
            panels::overview_tabs(mono)
                == [
                    OverviewTab::Habits,
                    OverviewTab::Documents,
                    OverviewTab::Subagents
                ]
        );
    });
    // A coding Mono keeps whatever overview tab was open.
    s.manager.update(visual, |m, cx| {
        m.select_overview(OverviewTab::Documents, cx)
    });
    open(visual, &s.id);
    s.manager.update(visual, |m, _| {
        let mono = m.roster.get(&s.id).unwrap();
        assert!(m.shown_overview_tab(mono) == OverviewTab::Documents);
        assert!(panels::overview_tabs(mono).contains(&OverviewTab::Sessions));
    });
}

#[gpui::test]
fn a_cited_paper_opens_in_the_documents_panel_not_a_code_pane(cx: &mut TestAppContext) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let dir = std::env::temp_dir().join(format!("monobot-paper-route-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("paper.pdf");
    std::fs::write(&path, b"%PDF-1.4\n").unwrap();
    let path = path.to_string_lossy().into_owned();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    s.window
        .update(visual, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.open(&s.id, w, cx))
        })
        .unwrap();
    s.manager.update(visual, |m, cx| {
        m.open_paper(
            path.clone(),
            3,
            Some("a passage".into()),
            "A paper".into(),
            cx,
        );
        assert_eq!(m.route, Some(Route::Paper(path.clone())));
        assert!(m.overview_tab == OverviewTab::Documents);
        assert!(m.loading_papers.contains(&path));
    });
    visual.run_until_parked();
    s.manager.update(visual, |m, _| {
        assert!(m.loading_papers.is_empty());
        assert_eq!(m.papers[&path].title, "A paper");
        assert_eq!(m.route, Some(Route::Paper(path.clone())));
    });
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn the_papers_a_mono_downloaded_wait_in_a_folder_of_their_request(cx: &mut TestAppContext) {
    use monocode_core::{tool::ToolCall, transcript::Row};
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let dir = std::env::temp_dir().join(format!("monobot-paper-folder-{}", mono::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("airGR and airGRteaching.pdf");
    std::fs::write(&path, b"%PDF-1.4\n").unwrap();
    let path = path.to_string_lossy().into_owned();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    s.window
        .update(visual, |_, w, cx| {
            s.manager.update(cx, |m, cx| {
                m.open(&s.id, w, cx);
                m.dismiss_settings(w, cx);
            })
        })
        .unwrap();
    let mut download = ToolCall::new("t1", "web.download");
    download.input = r#"{"url":"https://example.com/airgr"}"#.into();
    download.output =
        serde_json::json!({ "path": path, "url": "https://x", "bytes": 9 }).to_string();
    let row = |role: &str, text: String| Row {
        role: role.into(),
        text,
        first: true,
        sent_at: Some(1),
    };
    s.manager.update(visual, |m, cx| {
        m.chats[&s.id].update(cx, |view, cx| {
            view.transcript.update(cx, |t, cx| {
                let start = t.source_rows().len();
                t.patch(
                    start,
                    vec![
                        row("user", "Paper de diseño hidrológico con R".into()),
                        row("tool", serde_json::to_string(&download).unwrap()),
                        row("assistant", format!("PDF en `{path}`.")),
                    ],
                    cx,
                );
            });
        });
        m.select_overview(OverviewTab::Documents, cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    let folder = s
        .manager
        .read_with(visual, |m, cx| m.document_folders(&s.id, cx));
    assert_eq!(folder.len(), 1);
    assert_eq!(folder[0].title, "Paper de diseño hidrológico con R");
    // Debug selectors are looked up by `&'static str`.
    let id = folder[0].id.clone();
    let file: &'static str = format!("mono-folder-file-{id}-0").leak();
    let folder: &'static str = format!("mono-folder-{id}").leak();
    assert!(
        visual.debug_bounds(folder).is_some(),
        "the request has a folder"
    );
    let row = visual
        .debug_bounds(file)
        .expect("the folder lists the paper");
    visual.simulate_click(row.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(
        s.manager.read_with(visual, |m, _| m.route.clone()),
        Some(Route::Paper(path.clone()))
    );
    // Back from the paper finds it where it was.
    visual.refresh().unwrap();
    let back = visual.debug_bounds("mono-page-back").unwrap();
    visual.simulate_click(back.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(s.manager.read_with(visual, |m, _| m.route.is_none()));
    assert!(visual.debug_bounds(file).is_some());
    // A folder closes and opens in place.
    let header = visual.debug_bounds(folder).unwrap();
    visual.simulate_click(header.center(), Default::default());
    visual.refresh().unwrap();
    assert!(visual.debug_bounds(file).is_none());
    visual.simulate_click(header.center(), Default::default());
    visual.refresh().unwrap();
    assert!(visual.debug_bounds(file).is_some());
    let _ = std::fs::remove_dir_all(dir);
}

#[gpui::test]
fn activity_follows_the_latest_turn_and_lists_what_it_researched(cx: &mut TestAppContext) {
    use monocode_core::{tool::ToolCall, transcript::Row};
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    s.window
        .update(visual, |_, w, cx| {
            s.manager.update(cx, |m, cx| m.open(&s.id, w, cx))
        })
        .unwrap();
    let mut search = ToolCall::new("t1", "WebSearch");
    search.input = r#"{"query":"copper output"}"#.into();
    search.output = r#"Links: [{"title":"Mining report","url":"https://a.example/report"}]"#.into();
    let row = |role: &str, text: String, first: bool| Row {
        role: role.into(),
        text,
        first,
        sent_at: first.then_some(1),
    };
    s.manager.update(visual, |m, cx| {
        m.chats[&s.id].update(cx, |view, cx| {
            view.transcript.update(cx, |t, cx| {
                let start = t.source_rows().len();
                t.patch(
                    start,
                    vec![
                        row("user", "How much copper?".into(), true),
                        row("tool", serde_json::to_string(&search).unwrap(), true),
                        row(
                            "assistant",
                            "Output grew [1](https://a.example/report \"Mining report | Grew 4%\")."
                                .into(),
                            true,
                        ),
                    ],
                    cx,
                );
            });
        });
        m.select_overview(OverviewTab::Activity, cx);
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("mono-research").is_some(),
        "the latest turn's research shows without picking a message"
    );
}

#[gpui::test]
fn typing_at_in_a_group_lists_its_members(cx: &mut TestAppContext) {
    use monocode_core::mono::groups::Group;
    let s = setup(cx);
    cx.update(|cx| {
        crate::input::bind(cx);
        crate::composer_completion::bind(cx);
        crate::popover::bind(cx);
    });
    let handle: gpui::AnyWindowHandle = s.window.into();
    handle
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.dismiss_settings(window, cx);
                let root = m.root.clone();
                let lead = m
                    .roster
                    .create_bot(&root, "principal", false)
                    .unwrap()
                    .id
                    .clone();
                let finance = m
                    .roster
                    .create_bot(&root, "finance", false)
                    .unwrap()
                    .id
                    .clone();
                m.roster.get_mut(&finance).unwrap().name = "Cash".into();
                let group = m.roster.get_mut(&s.id).unwrap();
                group.kind = Some("group".into());
                group.name = "Team".into();
                group.group = Some(Group {
                    members: vec![lead, finance],
                    ..Group::default()
                });
                m.configure_chat(&s.id, cx);
                m.focus_active(window, cx);
            })
        })
        .unwrap();
    cx.run_until_parked();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.simulate_input("@");
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual.debug_bounds("composer-completion-list").is_some(),
        "the @ menu opens"
    );
    assert!(visual.debug_bounds("completion-person-Cash").is_some());
    visual.simulate_input("Ca");
    visual.run_until_parked();
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    let chat = visual.update(|_, cx| s.manager.read(cx).chats[&s.id].clone());
    let text = visual.update(|_, cx| chat.read(cx).input.read(cx).content.clone());
    assert_eq!(text, "@Cash ");
}

#[gpui::test]
fn resting_on_a_citation_moves_only_the_paper_on_screen_and_records_show_their_abstract(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    cx.update(|cx| cx.set_reduce_motion(true));
    let dir = std::env::temp_dir().join(format!("monobot-paper-point-{}", mono::new_id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("paper.pdf");
    std::fs::write(&path, b"%PDF-1.4\n").unwrap();
    let path = path.to_string_lossy().into_owned();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    s.window
        .update(visual, |_, w, cx| {
            s.manager.update(cx, |m, cx| {
                m.open(&s.id, w, cx);
                m.dismiss_settings(w, cx);
            })
        })
        .unwrap();
    s.manager.update(visual, |m, cx| {
        m.open_paper(path.clone(), 1, None, "A paper".into(), cx)
    });
    visual.run_until_parked();
    s.manager.update(visual, |m, cx| {
        assert!(m.point_open_paper(&path, 1, Some("a passage".into()), cx));
        assert!(!m.point_open_paper("/elsewhere.pdf", 1, None, cx));
        m.route = None;
        assert!(
            !m.point_open_paper(&path, 1, None, cx),
            "a paper not on screen is left alone"
        );
        let meta: monocode_core::papers::PaperMeta = serde_json::from_value(serde_json::json!({
            "paper": {
                "key": "10.1/x", "title": "Rivers", "authors": ["Ana Ríos"], "pdfUrls": [],
                "access": "closed",
                "abstract": "We show that the reported result preserves the complete data."
            },
            "availability": "abstract"
        }))
        .unwrap();
        m.paper_records.insert(
            "https://doi.org/10.1/x".into(),
            super::papers::PaperRecord {
                meta: Some(meta),
                passage: Some("the reported result preserves the complete data".into()),
            },
        );
        m.route = Some(Route::PaperRecord("https://doi.org/10.1/x".into()));
        m.show_overview(cx);
        cx.notify();
    });
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("paper-record").is_some());
    let _ = std::fs::remove_dir_all(dir);
}

/// Makes the test project a git repository with one commit, so a workstream
/// can give its orchestrator a worktree.
fn git_project(project: &str) {
    let git = |args: &[&str]| {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(project)
            .output()
            .unwrap();
        assert!(status.status.success(), "git {args:?}: {status:?}");
    };
    git(&["init", "-b", "main"]);
    git(&["config", "user.name", "Native test"]);
    git(&["config", "user.email", "native@example.invalid"]);
    git(&["config", "commit.gpgsign", "false"]);
    git(&["commit", "--allow-empty", "-m", "Base"]);
}

/// The Code zone with no Mono open: the rail lists the Monos and Workstreams.
fn show_rail(s: &Setup, cx: &mut TestAppContext) {
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        input::bind(cx);
        crate::popover::bind(cx);
        s.manager.update(cx, |m, cx| {
            m.settings = None;
            m.close(cx);
            cx.notify();
        });
    });
}

fn click(visual: &mut gpui::VisualTestContext, selector: &'static str) {
    let bounds = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{selector} is not on screen"));
    visual.simulate_click(bounds.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
}

#[gpui::test]
fn new_workstream_from_the_rail_starts_an_orchestrator_in_its_own_worktree(
    cx: &mut TestAppContext,
) {
    let s = setup(cx);
    git_project(&s.project);
    show_rail(&s, cx);
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();

    // The first time, the tour; Escape skips it and it never shows again.
    click(visual, "new-workstream");
    assert!(visual.debug_bounds("workstream-intro").is_some());
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("workstream-intro").is_none());
    s.manager
        .read_with(visual, |m, _| assert!(m.roster.workstream_intro_seen));

    click(visual, "new-workstream");
    assert!(visual.debug_bounds("workstream-intro").is_none());
    assert!(visual.debug_bounds("new-workstream-dialog").is_some());
    let dialog = s.manager.read_with(visual, |m, _| {
        m.workstreams.dialog.as_ref().unwrap().0.clone()
    });
    // It starts on the sidebar's project, and needs a name first.
    dialog.read_with(visual, |d, cx| {
        assert_eq!(d.project, s.project);
        assert!(d.worktree);
        assert!(!d.ready(cx));
    });
    assert!(visual.debug_bounds("new-workstream-branches").is_none());
    click(visual, "new-workstream-start");
    dialog.read_with(visual, |d, _| assert!(!d.busy));

    click(visual, "new-workstream-name");
    visual.simulate_input("Migrar la API");
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("new-workstream-branches").is_some());
    dialog.read_with(visual, |d, cx| {
        assert_eq!(d.name.read(cx).content, "Migrar la API");
        assert!(d.ready(cx));
    });
    // Where the orchestrator runs is a choice; its own worktree is the default.
    click(visual, "new-workstream-checkout-project");
    dialog.read_with(visual, |d, _| assert!(!d.worktree));
    click(visual, "new-workstream-checkout-worktree");
    dialog.read_with(visual, |d, _| assert!(d.worktree));

    click(visual, "new-workstream-start");
    assert!(visual.debug_bounds("new-workstream-dialog").is_none());
    let (id, workstream) = s.manager.read_with(visual, |m, _| {
        let mono = m.current().expect("the orchestrator opens");
        assert!(mono.is_workstream());
        assert_eq!(mono.name, "Migrar la API");
        assert_eq!(mono.projects, std::slice::from_ref(&s.project));
        assert!(m.overview_tab == OverviewTab::Sessions);
        // No goal, so nothing is sent: the user explains it in the chat.
        assert!(mono.outbox.is_empty());
        (mono.id.clone(), mono.workstream.clone().unwrap())
    });
    assert_eq!(workstream.slug, "migrar-la-api");
    assert_ne!(workstream.cwd, s.project);
    assert!(std::path::Path::new(&workstream.folder).is_dir());
    assert!(workstream.folder.ends_with(&format!("workstreams/{id}")));
    let branches = std::process::Command::new("git")
        .args(["branch", "--list", "ws/migrar-la-api/orchestrator"])
        .current_dir(&s.project)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&branches.stdout).contains("ws/migrar-la-api/orchestrator"));
    // Its chat starts in its own checkout, and it is saved.
    visual.update(|_, cx| {
        let m = s.manager.read(cx);
        assert_eq!(m.chats[&id].read(cx).summary.cwd, workstream.cwd);
    });
    let saved = Roster::load(&locked(&s.store).unwrap()).unwrap();
    assert_eq!(
        saved.get(&id).unwrap().workstream.as_ref(),
        Some(&workstream)
    );
    assert!(saved.workstream_intro_seen);

    // The same name again: the branch is taken, and the dialog says so.
    show_rail(&s, visual);
    visual.run_until_parked();
    visual.refresh().unwrap();
    click(visual, "sidebar-new-workstream");
    click(visual, "new-workstream-name");
    visual.simulate_input("Migrar la API");
    visual.run_until_parked();
    click(visual, "new-workstream-start");
    assert!(visual.debug_bounds("new-workstream-error").is_some());
    let dialog = s.manager.read_with(visual, |m, _| {
        m.workstreams.dialog.as_ref().unwrap().0.clone()
    });
    dialog.read_with(visual, |d, _| {
        assert!(!d.busy);
        assert!(d.error.is_some());
    });
    let _ = std::fs::remove_dir_all(&s.project);
}

#[gpui::test]
fn workstream_tour_steps_forward_and_back_then_opens_the_dialog(cx: &mut TestAppContext) {
    let s = setup(cx);
    show_rail(&s, cx);
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    click(visual, "new-workstream");
    let step = |visual: &mut gpui::VisualTestContext| {
        s.manager.read_with(visual, |m, cx| {
            m.workstreams.intro.as_ref().unwrap().0.read(cx).step
        })
    };
    assert_eq!(step(visual), 0);
    assert!(visual.debug_bounds("workstream-intro-back").is_none());
    click(visual, "workstream-intro-next");
    click(visual, "workstream-intro-next");
    assert_eq!(step(visual), 2);
    click(visual, "workstream-intro-back");
    assert_eq!(step(visual), 1);
    click(visual, "workstream-intro-next");
    click(visual, "workstream-intro-next");
    assert_eq!(step(visual), workstreams::STEPS.len() - 1);
    // The last step's button starts one.
    click(visual, "workstream-intro-next");
    assert!(visual.debug_bounds("workstream-intro").is_none());
    assert!(visual.debug_bounds("new-workstream-dialog").is_some());
    s.manager
        .read_with(visual, |m, _| assert!(m.roster.workstream_intro_seen));
    // Cancel closes it without making anything.
    let before = s.manager.read_with(visual, |m, _| m.roster.monos.len());
    click(visual, "new-workstream-cancel");
    assert!(visual.debug_bounds("new-workstream-dialog").is_none());
    s.manager
        .read_with(visual, |m, _| assert_eq!(m.roster.monos.len(), before));
}

#[gpui::test]
fn a_workstream_goal_is_its_orchestrators_first_message(cx: &mut TestAppContext) {
    let s = setup(cx);
    let data = s.root.parent().unwrap().to_path_buf();
    let created = mono::workstreams::create(
        &data,
        &mono::workstreams::NewWorkstream {
            name: "Batch of bugs".into(),
            project: s.project.clone(),
            worktree: false,
        },
    )
    .unwrap();
    s.window
        .update(cx, |_, window, cx| {
            s.manager.update(cx, |m, cx| {
                m.finish_workstream(created, "Fix the five open crash reports.", window, cx);
                let mono = m.current().unwrap();
                assert!(mono.is_workstream());
                // The project itself when it has no worktree of its own.
                assert_eq!(mono.workstream.as_ref().unwrap().cwd, s.project);
                assert_eq!(mono.outbox.len(), 1);
                assert_eq!(mono.outbox[0].text, "Fix the five open crash reports.");
                assert!(!mono.outbox[0].internal);
                // Nothing reaches a real provider from a test.
                let id = mono.id.clone();
                m.roster.get_mut(&id).unwrap().outbox.clear();
            })
        })
        .unwrap();
}

#[gpui::test]
fn a_workstream_shows_its_agents_prs_and_shared_files(cx: &mut TestAppContext) {
    use mono::workstreams::{BranchPr, Workstream};
    let s = setup(cx);
    let folder = s.root.parent().unwrap().join("workstreams").join("ws-tabs");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("plan.md"), "# Plan").unwrap();
    std::fs::write(folder.join("findings.md"), "").unwrap();
    let workstream = Workstream {
        project: s.project.clone(),
        cwd: s.project.clone(),
        slug: "migrate".into(),
        folder: folder.to_string_lossy().into_owned(),
        created_at: 1,
    };
    let pr = |number, state: &str, draft, head: &str| BranchPr {
        number,
        title: format!("Part {number}"),
        url: format!("https://github.com/o/r/pull/{number}"),
        state: state.into(),
        is_draft: draft,
        head_ref_name: head.into(),
    };
    // One agent the orchestrator started, idle in its own session.
    let agent = locked(&s.store)
        .unwrap()
        .create("codex", &s.project)
        .unwrap()
        .id;
    show_rail(&s, cx);
    s.window
        .update(cx, |shell, window, cx| {
            shell.mono_track_sessions(vec![agent], cx);
            s.manager.update(cx, |m, cx| {
                m.roster.create_workstream(mono::workstreams::Created {
                    id: "ws-tabs".into(),
                    name: "Migrate".into(),
                    workstream: workstream.clone(),
                });
                m.roster
                    .get_mut("ws-tabs")
                    .unwrap()
                    .launches
                    .push(mono::Launch {
                        session_id: agent,
                        cwd: s.project.clone(),
                        title: "API agent".into(),
                        harness: "codex".into(),
                        model: None,
                        origin_turn: 0,
                        notify_on_complete: false,
                        completed: false,
                        dismissed: false,
                    });
                // What gh would list: two open, one draft, one merged.
                m.workstreams.prs.insert(
                    "ws-tabs".into(),
                    workstreams::Listing {
                        items: Some(Ok(vec![
                            pr(4, "OPEN", false, "ws/migrate/api"),
                            pr(3, "OPEN", true, "ws/migrate/web"),
                            pr(2, "MERGED", false, "ws/migrate/tests"),
                            pr(1, "OPEN", false, "ws/migrate/docs"),
                        ])),
                        loading: false,
                        loaded_at: Some(std::time::Instant::now()),
                    },
                );
                m.open("ws-tabs", window, cx);
                m.overview_tab = OverviewTab::Sessions;
                m.show_overview(cx);
            })
        })
        .unwrap();
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();

    // Listed under Workstreams, with its own tabs.
    assert!(visual.debug_bounds("mono-rail-ws-tabs").is_some());
    s.manager.read_with(visual, |m, _| {
        let mono = m.roster.get("ws-tabs").unwrap();
        assert!(
            panels::overview_tabs(mono)
                == [OverviewTab::Sessions, OverviewTab::Prs, OverviewTab::Files]
        );
        assert_eq!(m.open_pr_count(mono), 2);
    });
    // Agents: grouped by what needs the user first.
    assert!(visual.debug_bounds("mono-agents-Idle").is_some());
    let card: &'static str = Box::leak(format!("mono-launch-{agent}").into_boxed_str());
    assert!(visual.debug_bounds(card).is_some());

    click(visual, "mono-overview-Prs");
    s.manager
        .read_with(visual, |m, _| assert!(m.overview_tab == OverviewTab::Prs));
    for selector in [
        "workstream-prs-Open",
        "workstream-prs-Draft",
        "workstream-prs-Merged",
        "workstream-pr-4",
        "workstream-pr-1",
    ] {
        assert!(visual.debug_bounds(selector).is_some(), "{selector}");
    }
    assert!(visual.debug_bounds("workstream-prs-Closed").is_none());
    let open = visual.debug_bounds("workstream-prs-Open").unwrap();
    let draft = visual.debug_bounds("workstream-prs-Draft").unwrap();
    let merged = visual.debug_bounds("workstream-prs-Merged").unwrap();
    assert!(open.top() < draft.top() && draft.top() < merged.top());

    click(visual, "mono-overview-Files");
    s.manager
        .read_with(visual, |m, _| assert!(m.overview_tab == OverviewTab::Files));
    assert!(visual.debug_bounds("workstream-files-reveal").is_some());
    let files = s.manager.read_with(visual, |m, _| {
        match m.workstreams.files["ws-tabs"].items.as_ref() {
            Some(Ok(files)) => files.iter().map(|f| f.name.clone()).collect::<Vec<_>>(),
            other => panic!("files not read: {:?}", other.map(|r| r.is_ok())),
        }
    });
    assert_eq!(files, ["findings.md", "plan.md"]);
    assert!(visual.debug_bounds("workstream-file-1").is_some());

    // A note the orchestrator writes shows up on Refresh.
    std::fs::write(folder.join("notes.md"), "").unwrap();
    click(visual, "workstream-files-refresh");
    assert!(visual.debug_bounds("workstream-file-2").is_some());
}

#[gpui::test]
fn a_bots_mail_connect_opens_the_apps_dialog_and_the_user_picks_who_reads(cx: &mut TestAppContext) {
    let s = setup(cx);
    s.manager.update(cx, |m, _| {
        let mono = m.roster.monos.iter_mut().find(|m| m.id == s.id).unwrap();
        mono.kind = Some("bot".into());
        mono.name = "Finance".into();
    });
    let answer = call(
        &s,
        "connect-ana",
        "mail.connect",
        json!({"address":"ana@gmail.com","name":"Ana, pareja"}),
        cx,
    )
    .try_recv()
    .unwrap()
    .unwrap();
    assert_eq!(answer["opened"], true);
    assert!(
        answer.get("dialog").is_none(),
        "the agent never sees the dialog"
    );
    let visual = &mut gpui::VisualTestContext::from_window(s.window.into(), cx);
    visual.simulate_resize(gpui::size(px(1000.), px(760.)));
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("mail-connect-dialog").is_some());
    let form = s
        .manager
        .read_with(visual, |m, _| m.mail_connect.as_ref().unwrap().1.clone());
    // Kept for the bot that asked, until the user opens it to every agent.
    form.read_with(visual, |form, _| {
        assert_eq!(
            *form.readers(),
            crate::inbox::mail::connect::Readers::Only(vec![s.id.clone()])
        )
    });
    let all = visual
        .debug_bounds("mail-connect-readers-all")
        .expect("the readers row");
    visual.simulate_click(all.center(), Default::default());
    visual.run_until_parked();
    form.read_with(visual, |form, _| {
        assert_eq!(*form.readers(), crate::inbox::mail::connect::Readers::All)
    });
    visual.refresh().unwrap();
    let cancel = visual.debug_bounds("mail-connect-cancel").unwrap();
    visual.simulate_click(cancel.center(), Default::default());
    visual.run_until_parked();
    s.manager
        .read_with(visual, |m, _| assert!(m.mail_connect.is_none()));
}
