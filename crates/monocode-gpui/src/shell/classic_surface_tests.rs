//! Original-layout destinations cover the project sessions without changing
//! the remembered sidebar or destroying the conversation workspace.
use super::*;
use gpui::{TestAppContext, VisualTestContext, WindowHandle};

#[derive(Clone, Copy, Debug)]
enum Surface {
    Inbox,
    Notes,
    Automations,
    Search,
    Mono,
}

impl Surface {
    fn open(self, shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
        match self {
            Self::Inbox => shell.open_inbox(window, cx),
            Self::Notes => shell.open_notes(window, cx),
            Self::Automations => {
                // An empty in-memory service keeps this navigation test
                // independent of asynchronous service initialization.
                shell.automation_state.service = Some(Arc::new(Mutex::new(
                    monocode_core::automations::Service::open(std::path::Path::new(":memory:"))
                        .unwrap(),
                )));
                shell.open_automations(window, cx);
            }
            Self::Search => shell.open_app_search(window, cx),
            Self::Mono => {
                let project = shell.workspace.project.clone();
                let session = shell.workspace.active;
                shell.mono_manager().update(cx, |monos, cx| {
                    let id = monos.roster.create(vec![project]).id.clone();
                    let mono = monos.roster.get_mut(&id).unwrap();
                    mono.name = "Parity Mono".into();
                    mono.session_id = session;
                    monos.open(&id, window, cx);
                });
            }
        }
    }

    fn close(self, shell: &mut Shell, session: i64, window: &mut Window, cx: &mut Context<Shell>) {
        match self {
            Self::Inbox | Self::Notes | Self::Automations => shell.close_app_view(window, cx),
            Self::Search => shell.close_app_search(window, cx),
            // Returning to a conversation is the same route used by the
            // original project rail and by opening a Mono's launched session.
            Self::Mono => shell.open(session, window, cx),
        }
    }

    fn header(self) -> &'static str {
        match self {
            Self::Search => "app-search-header",
            Self::Mono => "classic-mono-title",
            _ => "classic-app-surface-header",
        }
    }

    fn icon_left(self, visual: &mut VisualTestContext) -> Pixels {
        let (label, offset) = match self {
            Self::Search => ("app-search-field", 22.),
            Self::Mono => ("original-mono-name", 24.),
            _ => ("classic-app-surface-title", 22.),
        };
        visual.debug_bounds(label).unwrap().left() - px(offset)
    }
}

fn setup(cx: &mut TestAppContext, classic: bool, boxy: bool) -> (WindowHandle<Shell>, i64) {
    cx.update(|cx| cx.set_reduce_motion(true));
    let (window, _, session) = super::tests::window(cx);
    cx.simulate_window_resize(window.into(), gpui::size(px(1500.), px(900.)));
    window
        .update(cx, |shell, window, cx| {
            shell.workspace.classic_layout = classic;
            shell.workspace.new_layout_boxy = boxy;
            shell.workspace.sidebar_visible = true;
            shell.workspace.project_rail_open = true;
            shell.workspace.project_rail_width = 220.;
            shell.workspace.sidebar_width = 260.;
            shell.workspace.right_panel_open = false;
            shell.open(session, window, cx);
            shell.layout_changed(cx);
        })
        .unwrap();
    cx.run_until_parked();
    (window, session)
}

#[gpui::test]
fn mono_hover_card_closes_in_both_sidebars_without_rebuilding_chat(cx: &mut TestAppContext) {
    for (classic, compact) in [(false, false), (true, false), (true, true)] {
        let (window, session) = setup(cx, classic, false);
        let (id, transcript) = window
            .update(cx, |shell, _, cx| {
                shell.workspace.project_rail_open = !compact;
                shell.workspace.project_rail_monos_pinned = true;
                shell.workspace.preferences.monos = true;
                let project = shell.workspace.project.clone();
                let id = shell.mono_manager().update(cx, |monos, cx| {
                    let id = monos.roster.create(vec![project]).id.clone();
                    monos.close(cx);
                    id
                });
                let transcript = shell.sessions[&session].read(cx).transcript.clone();
                shell.layout_changed(cx);
                (id, transcript)
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        let before = transcript.read_with(visual, |t, _| t.render_passes);
        let selector: &'static str = Box::leak(format!("mono-rail-{id}").into_boxed_str());
        let row = visual.debug_bounds(selector).expect("agent row renders");
        visual.simulate_mouse_move(row.center(), None, Default::default());
        visual.run_until_parked();
        let card = visual
            .debug_bounds("mono-preview")
            .expect("hover opens the painted card");
        visual.simulate_mouse_move(card.center(), None, Default::default());
        visual.run_until_parked();
        visual.simulate_mouse_move(gpui::point(px(5.), px(5.)), None, Default::default());
        visual.run_until_parked();
        visual
            .executor()
            .advance_clock(std::time::Duration::from_millis(150));
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("mono-preview").is_none(),
            "classic={classic}, compact={compact}: the painted card must close"
        );
        assert_eq!(
            transcript.read_with(visual, |t, _| t.render_passes),
            before,
            "hover must invalidate only its sidebar"
        );
    }
}

#[gpui::test]
fn original_project_plus_offers_local_and_remote_folders(cx: &mut TestAppContext) {
    let (window, _) = setup(cx, true, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    let click = |selector: &'static str, visual: &mut VisualTestContext| {
        visual.refresh().unwrap();
        let bounds = visual.debug_bounds(selector).unwrap();
        visual.simulate_click(bounds.center(), Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
    };
    assert!(!visual.did_prompt_for_paths());
    click("rail-open-project", visual);
    assert!(
        !visual.did_prompt_for_paths(),
        "the plus opens a menu before choosing a folder source"
    );
    assert!(visual.debug_bounds("menu-action-open-project").is_some());
    assert!(
        visual
            .debug_bounds("menu-action-open-remote-project")
            .is_some()
    );
    click("menu-action-open-project", visual);
    assert!(visual.did_prompt_for_paths());
    visual.simulate_path_prompt_response(|options| {
        assert!(options.directories);
        assert!(!options.files && !options.multiple);
        None
    });
    visual.run_until_parked();
    click("rail-open-project", visual);
    click("menu-action-open-remote-project", visual);
    assert!(visual.debug_bounds("remote-project-picker").is_some());
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    window
        .read_with(visual, |shell, _| {
            assert!(shell.menu.is_none());
            assert!(shell.remote_project_picker.is_none());
            assert_eq!(shell.workspace.project, "/work/monocode");
        })
        .unwrap();
}

#[gpui::test]
fn original_collapsed_picker_selects_monos_and_projects_with_search_and_keyboard(
    cx: &mut TestAppContext,
) {
    cx.update(crate::input::bind);
    let (window, session) = setup(cx, true, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    let ids = window
        .update(visual, |shell, _, cx| {
            let ids = shell.mono_manager().update(cx, |manager, cx| {
                let mut ids = Vec::new();
                for (name, project) in [
                    ("Alpha assistant", "/work/monocode"),
                    ("Beta helper", "/work/another-project"),
                ] {
                    let id = manager.roster.create(vec![project.into()]).id.clone();
                    manager.roster.get_mut(&id).unwrap().name = name.into();
                    ids.push(id);
                }
                cx.notify();
                ids
            });
            shell.workspace.project_rail_open = false;
            shell.layout_changed(cx);
            ids
        })
        .unwrap();
    visual.run_until_parked();
    let open = |visual: &mut VisualTestContext| {
        visual.refresh().unwrap();
        let mascot = visual.debug_bounds("rail-current-project").unwrap();
        visual.simulate_click(mascot.center(), Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("project-picker").is_some());
    };
    let alpha: &'static str = Box::leak(format!("project-picker-mono-{}", ids[0]).into_boxed_str());
    let beta: &'static str = Box::leak(format!("project-picker-mono-{}", ids[1]).into_boxed_str());
    open(visual);
    let alpha_row = visual.debug_bounds(alpha).unwrap();
    let beta_row = visual.debug_bounds(beta).unwrap();
    let project_row = visual.debug_bounds("project-picker-project-0").unwrap();
    assert!(alpha_row.top() < beta_row.top() && beta_row.top() < project_row.top());
    // The menu gives the picker its height; without it the card collapses to
    // its padding and every row falls outside it, so clicks dismiss it.
    let picker = visual.debug_bounds("project-picker").unwrap();
    assert!(
        picker.top() < alpha_row.top() && project_row.bottom() <= picker.bottom(),
        "rows sit inside the picker: {picker:?}"
    );
    visual.simulate_input("bEtA");
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds(alpha).is_none());
    assert!(visual.debug_bounds(beta).is_some());
    assert!(visual.debug_bounds("project-picker-project-0").is_none());
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    window
        .read_with(visual, |shell, cx| {
            assert!(shell.menu.is_none());
            assert_eq!(shell.mono_manager().read(cx).active.as_ref(), Some(&ids[1]));
            assert_eq!(shell.workspace.project, "/work/monocode");
            assert!(
                shell
                    .visible_tabs(cx)
                    .iter()
                    .all(|tab| !matches!(tab, TitleTab::Mono { .. }))
            );
        })
        .unwrap();
    open(visual);
    visual.simulate_keystrokes("down down enter");
    visual.run_until_parked();
    window
        .read_with(visual, |shell, cx| {
            assert!(shell.menu.is_none());
            assert!(!shell.mono_active(cx));
            assert_eq!(shell.workspace.active, Some(session));
            assert!(!shell.workspace.project_rail_open);
        })
        .unwrap();
    open(visual);
    let row = visual.debug_bounds(alpha).unwrap();
    visual.simulate_click(row.center(), Default::default());
    visual.run_until_parked();
    window
        .read_with(visual, |shell, cx| {
            assert_eq!(shell.mono_manager().read(cx).active.as_ref(), Some(&ids[0]));
        })
        .unwrap();
}

fn assert_original_surface(surface: Surface, cx: &mut TestAppContext) {
    let (window, session) = setup(cx, true, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("sidebar-content").is_some());
    assert!(visual.debug_bounds("title-tab-strip").is_some());
    let (tabs, conversation) = window
        .read_with(visual, |shell, cx| {
            (
                shell.workspace.tabs.clone(),
                shell.sessions[&session].read(cx).input.entity_id(),
            )
        })
        .unwrap();

    window
        .update(visual, |shell, window, cx| surface.open(shell, window, cx))
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();

    assert!(
        visual.debug_bounds("sidebar-content").is_none(),
        "{surface:?} covers the original sessions sidebar"
    );
    assert!(
        visual.debug_bounds("title-tab-strip").is_none(),
        "{surface:?} replaces conversation tabs with its own header"
    );
    let rail = visual.debug_bounds("project-rail-frame").unwrap();
    assert_eq!(rail.size.width, px(220.));
    let header = visual.debug_bounds(surface.header()).unwrap();
    assert_eq!(header.top(), px(0.));
    assert_eq!(header.size.height, px(40.));
    assert!(header.left() >= rail.right());
    assert_eq!(surface.icon_left(visual), rail.right() + px(12.));
    if matches!(surface, Surface::Search) {
        assert!(visual.debug_bounds("classic-app-surface-header").is_none());
    }
    window
        .read_with(visual, |shell, cx| {
            assert!(shell.workspace.sidebar_visible);
            assert!(shell.workspace.project_rail_open);
            assert_eq!(shell.workspace.tabs, tabs);
            assert_eq!(shell.workspace.active, Some(session));
            assert_eq!(
                shell.sessions[&session].read(cx).input.entity_id(),
                conversation,
                "opening a destination retains the current conversation and draft field"
            );
        })
        .unwrap();

    window
        .update(visual, |shell, window, cx| {
            surface.close(shell, session, window, cx)
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(visual.debug_bounds("sidebar-content").is_some());
    assert!(visual.debug_bounds("title-tab-strip").is_some());
    assert!(visual.debug_bounds("classic-app-surface-header").is_none());
    assert!(visual.debug_bounds("classic-mono-title").is_none());
    window
        .read_with(visual, |shell, _| {
            assert!(shell.workspace.sidebar_visible);
            assert_eq!(shell.workspace.sidebar_width, 260.);
            assert_eq!(shell.workspace.tabs, tabs);
        })
        .unwrap();
}

#[gpui::test]
fn original_app_surfaces_inbox_uses_own_header_and_restores_sessions(cx: &mut TestAppContext) {
    assert_original_surface(Surface::Inbox, cx);
}

#[gpui::test]
fn original_app_surfaces_notes_uses_own_header_and_restores_sessions(cx: &mut TestAppContext) {
    assert_original_surface(Surface::Notes, cx);
}

#[gpui::test]
fn original_app_surfaces_automations_uses_own_header_and_restores_sessions(
    cx: &mut TestAppContext,
) {
    assert_original_surface(Surface::Automations, cx);
}

#[gpui::test]
fn original_app_surfaces_search_uses_search_field_header_and_restores_sessions(
    cx: &mut TestAppContext,
) {
    assert_original_surface(Surface::Search, cx);
}

#[gpui::test]
fn original_app_surfaces_mono_uses_identity_header_and_restores_sessions(cx: &mut TestAppContext) {
    assert_original_surface(Surface::Mono, cx);
}

#[gpui::test]
fn original_app_surfaces_mono_overview_and_rail_back_use_original_controls(
    cx: &mut TestAppContext,
) {
    let (window, session) = setup(cx, true, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |shell, window, cx| {
            Surface::Mono.open(shell, window, cx)
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    let overview = visual.debug_bounds("classic-mono-overview").unwrap();
    visual.simulate_click(overview.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();

    let header = visual
        .debug_bounds("original-mono-overview-header")
        .unwrap();
    assert_eq!(header.top(), px(0.));
    assert_eq!(header.size.height, px(40.));
    assert!(visual.debug_bounds("mono-overview-tabs").is_some());
    let first_tab = visual.debug_bounds("mono-overview-Overview").unwrap();
    assert_eq!(first_tab.left() - header.left(), px(8.));
    assert!((first_tab.center().y - header.center().y).abs() <= px(0.5));
    assert!(visual.debug_bounds("sidebar-content").is_none());
    assert!(visual.debug_bounds("title-tab-strip").is_none());
    window
        .read_with(visual, |shell, cx| {
            assert!(shell.mono_active(cx));
            assert!(shell.workspace.right_panel_open);
            assert!(shell.workspace.sidebar_visible);
        })
        .unwrap();

    let hide = visual.debug_bounds("original-hide-mono-overview").unwrap();
    visual.simulate_click(hide.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    assert!(
        visual
            .debug_bounds("original-mono-overview-header")
            .is_none()
    );
    window
        .read_with(visual, |shell, cx| {
            assert!(!shell.workspace.right_panel_open);
            assert!(shell.mono_active(cx), "hiding overview keeps the Mono open");
        })
        .unwrap();

    let back = visual.debug_bounds("visit-back").unwrap();
    visual.simulate_click(back.center(), Default::default());
    visual.run_until_parked();
    visual.refresh().unwrap();
    window
        .read_with(visual, |shell, cx| {
            assert!(!shell.mono_active(cx));
            assert_eq!(shell.workspace.active, Some(session));
            assert!(shell.workspace.sidebar_visible);
            assert!(shell.workspace.project_rail_open);
            assert_eq!(shell.workspace.sidebar_width, 260.);
        })
        .unwrap();
    assert!(visual.debug_bounds("classic-mono-title").is_none());
    assert!(visual.debug_bounds("title-tab-strip").is_some());
    assert!(visual.debug_bounds("sidebar-content").is_some());
}

#[gpui::test]
fn project_clicks_leave_monos_in_original_layout(cx: &mut TestAppContext) {
    for boxy in [false, true] {
        let (window, _) = setup(cx, true, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let mut targets = Vec::new();
        for path in ["/work/monocode", "/work/another-project"] {
            let target = window
                .update(visual, |shell, window, cx| {
                    shell.select_project(path.into(), window, cx);
                    Surface::Mono.open(shell, window, cx);
                    let ix = shell
                        .workspace
                        .projects
                        .iter()
                        .position(|p| p == path)
                        .unwrap();
                    let mono = shell.mono_manager().read(cx).active.clone().unwrap();
                    let project_selector = format!("rail-project-button-{ix}");
                    (
                        path,
                        Box::leak(project_selector.into_boxed_str()) as &'static str,
                        Box::leak(format!("mono-rail-{mono}").into_boxed_str()) as &'static str,
                        mono,
                    )
                })
                .unwrap();
            visual.run_until_parked();
            targets.push(target);
        }
        // Visit both projects repeatedly after each has remembered a Mono.
        // Exercise the real rail/space buttons, including a same-project click.
        for (path, project_selector, mono_selector, mono) in targets.iter().cycle().take(4) {
            for reopen in [true, false] {
                visual.refresh().unwrap();
                let project = visual.debug_bounds(project_selector).unwrap();
                visual.simulate_click(project.center(), Default::default());
                visual.run_until_parked();
                visual.refresh().unwrap();
                window
                    .read_with(visual, |shell, cx| {
                        assert_eq!(&shell.workspace.project, path);
                        assert!(
                            !shell.mono_active(cx),
                            "original-layout project clicks leave the Mono (boxy={boxy})"
                        );
                        let session = shell.workspace.active.expect("the project shows a session");
                        assert_eq!(shell.session_metadata(session, cx).summary.cwd, *path);
                    })
                    .unwrap();
                if reopen {
                    let row = visual.debug_bounds(mono_selector).unwrap();
                    visual.simulate_click(row.center(), Default::default());
                    visual.run_until_parked();
                    window
                        .read_with(visual, |shell, cx| {
                            assert_eq!(shell.mono_manager().read(cx).active.as_ref(), Some(mono));
                        })
                        .unwrap();
                }
            }
        }
    }
}

#[gpui::test]
fn new_layout_projects_restore_the_last_session_or_mono_tab(cx: &mut TestAppContext) {
    for boxy in [false, true] {
        let (window, _) = setup(cx, false, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let stable = |value: String| Box::leak(value.into_boxed_str()) as &'static str;
        let mut targets = Vec::new();
        for path in ["/work/monocode", "/work/another-project"] {
            let target = window
                .update(visual, |shell, window, cx| {
                    shell.select_project(path.into(), window, cx);
                    let session = shell.workspace.active.unwrap();
                    Surface::Mono.open(shell, window, cx);
                    let mono = shell.mono_manager().read(cx).active.clone().unwrap();
                    let ix = shell
                        .workspace
                        .projects
                        .iter()
                        .position(|p| p == path)
                        .unwrap();
                    (stable(format!("project-space-{ix}")), session, mono)
                })
                .unwrap();
            visual.run_until_parked();
            targets.push(target);
        }
        let click = |selector: &'static str, visual: &mut VisualTestContext| {
            visual.refresh().unwrap();
            let bounds = visual.debug_bounds(selector).unwrap();
            visual.simulate_click(bounds.center(), Default::default());
            visual.run_until_parked();
            visual.refresh().unwrap();
        };
        let assert_mono = |ix: usize, visual: &mut VisualTestContext| {
            window
                .read_with(visual, |shell, cx| {
                    assert_eq!(
                        shell.mono_manager().read(cx).active.as_ref(),
                        Some(&targets[ix].2)
                    );
                    assert!(
                        shell
                            .visible_tabs(cx)
                            .contains(&shell.mono_tab(targets[ix].2.clone(), cx))
                    );
                })
                .unwrap();
            assert!(
                visual
                    .debug_bounds(stable(format!("mono-title-tab-{}", targets[ix].2)))
                    .is_some()
            );
        };
        click(targets[0].0, visual);
        assert_mono(0, visual);
        click(stable(format!("title-tab-{}", targets[0].1)), visual);
        click(targets[1].0, visual);
        assert_mono(1, visual);
        click(targets[0].0, visual);
        window
            .read_with(visual, |shell, cx| {
                assert!(!shell.mono_active(cx));
                assert_eq!(shell.workspace.active, Some(targets[0].1));
            })
            .unwrap();
        click(stable(format!("mono-rail-{}", targets[0].2)), visual);
        assert_mono(0, visual);
        // Both same-project clicks and returning from another project restore
        // a selected Mono in either new frame style.
        click(targets[0].0, visual);
        assert_mono(0, visual);
        click(targets[1].0, visual);
        assert_mono(1, visual);
        click(targets[0].0, visual);
        assert_mono(0, visual);
    }
}

#[gpui::test]
fn original_monos_leave_project_tabs_unchanged_and_close_as_surfaces(cx: &mut TestAppContext) {
    cx.update(super::bind_keys);
    for boxy in [false, true] {
        let (window, session) = setup(cx, true, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        let (order, focus, mono, project_selector) = window
            .update(visual, |shell, window, cx| {
                let order = shell.workspace.title_order.clone();
                let focus = shell.workspace.scope_focus.clone();
                Surface::Mono.open(shell, window, cx);
                let mono = shell.mono_manager().read(cx).active.clone().unwrap();
                let ix = shell
                    .workspace
                    .projects
                    .iter()
                    .position(|p| p == &shell.workspace.project)
                    .unwrap();
                (
                    order,
                    focus,
                    mono,
                    Box::leak(format!("rail-project-button-{ix}").into_boxed_str()) as &'static str,
                )
            })
            .unwrap();
        visual.run_until_parked();
        window
            .read_with(visual, |shell, cx| {
                assert_eq!(shell.workspace.title_order, order);
                assert_eq!(shell.workspace.scope_focus, focus);
                assert!(
                    shell
                        .visible_tabs(cx)
                        .iter()
                        .all(|tab| !matches!(tab, TitleTab::Mono { .. }))
                );
            })
            .unwrap();
        let mono_selector = Box::leak(format!("mono-title-tab-{mono}").into_boxed_str());
        visual.refresh().unwrap();
        let project = visual.debug_bounds(project_selector).unwrap();
        visual.simulate_click(project.center(), Default::default());
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("title-tab-strip").is_some());
        assert!(visual.debug_bounds(mono_selector).is_none());

        window
            .update(visual, |shell, window, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.open(&mono, window, cx));
            })
            .unwrap();
        visual.run_until_parked();
        visual.simulate_keystrokes("cmd-w");
        visual.run_until_parked();
        window
            .read_with(visual, |shell, cx| {
                assert!(!shell.mono_active(cx));
                assert_eq!(shell.workspace.active, Some(session));
                assert_eq!(shell.workspace.title_order, order);
            })
            .unwrap();

        window
            .update(visual, |shell, window, cx| {
                shell
                    .mono_manager()
                    .update(cx, |m, cx| m.open(&mono, window, cx));
                shell.toggle_layout(cx);
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(
            visual.debug_bounds(mono_selector).is_some(),
            "switching to either new layout gives the open Mono a tab"
        );
    }
}

#[gpui::test]
fn original_layout_hides_saved_mono_tabs_until_returning_to_new_layout(cx: &mut TestAppContext) {
    for boxy in [false, true] {
        let (window, session) = setup(cx, false, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        window
            .update(visual, |shell, window, cx| {
                Surface::Mono.open(shell, window, cx)
            })
            .unwrap();
        visual.run_until_parked();
        let (mono_tab, mono_selector) = window
            .update(visual, |shell, _, cx| {
                let id = shell.mono_manager().read(cx).active.clone().unwrap();
                let tab = shell.mono_tab(id.clone(), cx);
                assert!(shell.visible_tabs(cx).contains(&tab));
                shell.toggle_layout(cx);
                (
                    tab,
                    Box::leak(format!("mono-title-tab-{id}").into_boxed_str()) as &'static str,
                )
            })
            .unwrap();
        visual.run_until_parked();
        window
            .update(visual, |shell, window, cx| {
                assert!(shell.workspace.title_order.contains(&mono_tab));
                assert!(!shell.visible_tabs(cx).contains(&mono_tab));
                shell.open(session, window, cx);
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("title-tab-strip").is_some());
        assert!(visual.debug_bounds(mono_selector).is_none());
        window
            .update(visual, |shell, _, cx| shell.toggle_layout(cx))
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds(mono_selector).is_some());
    }
}

#[gpui::test]
fn original_app_surfaces_keep_icon_rail_and_remember_closed_sessions(cx: &mut TestAppContext) {
    let (window, session) = setup(cx, true, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |shell, _, cx| {
            shell.workspace.project_rail_open = false;
            shell.workspace.sidebar_visible = false;
            shell.workspace.preferences.theme = "dark".into();
            shell.workspace.preferences.sidebar_opacity = 80;
            shell.workspace.preferences.body_glass = true;
            shell.layout_changed(cx);
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();
    let sessions_rail = visual.debug_bounds("project-rail-frame").unwrap();
    let sessions_picker = visual.debug_bounds("rail-current-project").unwrap();
    let sessions_toggle = visual.debug_bounds("toggle-sidebar").unwrap();
    let sessions_back = visual.debug_bounds("visit-back").unwrap();
    let sessions_forward = visual.debug_bounds("visit-forward").unwrap();
    assert_eq!(sessions_back.left(), sessions_toggle.right());
    assert_eq!(sessions_forward.right(), px(TRAFFIC_LIGHTS + 72.));
    for surface in [
        Surface::Inbox,
        Surface::Notes,
        Surface::Automations,
        Surface::Search,
        Surface::Mono,
    ] {
        window
            .update(visual, |shell, window, cx| surface.open(shell, window, cx))
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        let rail = visual.debug_bounds("project-rail-frame").unwrap();
        assert_eq!(rail.size.width, px(48.));
        assert_eq!(
            rail, sessions_rail,
            "{surface:?} keeps the rail below the title bar"
        );
        #[cfg(target_os = "macos")]
        {
            let background = visual.debug_bounds("classic-surface-background").unwrap();
            assert_eq!(background.top(), rail.top());
            assert_eq!(background.bottom(), rail.bottom());
            window
                .update(visual, |_, window, _| {
                    let scale = window.scale_factor();
                    let quads = window.painted_quads();
                    let painted_at = |color: gpui::Hsla, x: f32, y: f32| {
                        let point = gpui::point(
                            gpui::ScaledPixels(x * scale),
                            gpui::ScaledPixels(y * scale),
                        );
                        quads.iter().any(|quad| {
                            quad.background == color.into()
                                && quad.bounds.contains(&point)
                                && quad.content_mask.bounds.contains(&point)
                        })
                    };
                    assert_ne!(shell_fill(), main_pane_fill());
                    // Test the actual wash on both sides of the rail edge, which
                    // passes between the native traffic lights at x=48.
                    for x in [24., 60.] {
                        assert!(painted_at(shell_fill(), x, 20.));
                        assert!(
                            !painted_at(main_pane_fill(), x, 20.),
                            "{surface:?} paints content tint behind the traffic lights"
                        );
                    }
                    assert!(
                        painted_at(main_pane_fill(), 60., 50.),
                        "the destination body retains its content tint"
                    );
                })
                .unwrap();
        }
        assert_eq!(
            visual.debug_bounds("rail-current-project").unwrap(),
            sessions_picker,
            "{surface:?} keeps collapsed rail controls in place"
        );
        assert!(visual.debug_bounds("rail-expand-projects").is_none());
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        assert_eq!(toggle, sessions_toggle);
        let forward = visual.debug_bounds("visit-forward").unwrap();
        assert_eq!(visual.debug_bounds("visit-back").unwrap(), sessions_back);
        assert_eq!(forward, sessions_forward);
        assert_eq!(surface.icon_left(visual), forward.right() + px(6.));
        let main = visual.debug_bounds("main-pane-frame").unwrap();
        assert_eq!(main.left(), rail.right());
        assert!(visual.debug_bounds("sidebar-content").is_none());
        assert!(visual.debug_bounds("title-tab-strip").is_none());
        let header = visual.debug_bounds(surface.header()).unwrap();
        assert_eq!(header.top(), px(0.));
        assert_eq!(header.size.height, px(40.));
        window
            .read_with(visual, |shell, _| {
                assert!(!shell.workspace.sidebar_visible);
                assert!(!shell.workspace.project_rail_open);
            })
            .unwrap();
        // The one titlebar button remains usable on every destination.
        for open in [true, false] {
            let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
            visual.simulate_click(toggle.center(), Default::default());
            visual.run_until_parked();
            visual.refresh().unwrap();
            assert_eq!(
                visual.debug_bounds("toggle-sidebar").unwrap(),
                sessions_toggle
            );
            assert_eq!(
                visual
                    .debug_bounds("project-rail-frame")
                    .unwrap()
                    .size
                    .width,
                px(if open { 220. } else { 48. })
            );
            assert!(visual.debug_bounds("rail-expand-projects").is_none());
        }
        window
            .update(visual, |shell, window, cx| {
                surface.close(shell, session, window, cx)
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        assert!(visual.debug_bounds("sidebar-content").is_none());
        assert!(visual.debug_bounds("title-tab-strip").is_some());
        window
            .read_with(visual, |shell, _| {
                assert!(!shell.workspace.sidebar_visible);
                assert!(!shell.workspace.project_rail_open);
                assert_eq!(shell.workspace.sidebar_width, 260.);
            })
            .unwrap();
    }
}

#[gpui::test]
fn original_app_surfaces_leave_both_new_layout_styles_tabbed(cx: &mut TestAppContext) {
    for boxy in [false, true] {
        let (window, session) = setup(cx, false, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        for surface in [
            Surface::Inbox,
            Surface::Notes,
            Surface::Automations,
            Surface::Search,
            Surface::Mono,
        ] {
            window
                .update(visual, |shell, window, cx| surface.open(shell, window, cx))
                .unwrap();
            visual.run_until_parked();
            visual.refresh().unwrap();
            assert!(
                visual.debug_bounds("sidebar-content").is_some(),
                "{surface:?} keeps the new layout sidebar (boxy={boxy})"
            );
            assert!(
                visual.debug_bounds("title-tab-strip").is_some(),
                "{surface:?} stays a workspace tab (boxy={boxy})"
            );
            let app = match surface {
                Surface::Inbox => Some(app_views::AppView::Inbox),
                Surface::Notes => Some(app_views::AppView::Notes),
                Surface::Automations => Some(app_views::AppView::Automations),
                _ => None,
            };
            if let Some(app) = app {
                let selector = Box::leak(format!("app-title-tab-{}", app.label()).into_boxed_str());
                assert!(
                    visual.debug_bounds(selector).is_some(),
                    "the destination itself has a tab (boxy={boxy})"
                );
                window
                    .read_with(visual, |shell, cx| {
                        assert!(shell.visible_tabs(cx).contains(&TitleTab::App(app)));
                        assert_eq!(shell.active_title_tab(cx), Some(TitleTab::App(app)));
                    })
                    .unwrap();
            }
            assert!(visual.debug_bounds("classic-app-surface-header").is_none());
            assert!(visual.debug_bounds("classic-mono-title").is_none());
            assert!(visual.debug_bounds("project-rail-frame").is_none());
            window
                .read_with(visual, |shell, _| {
                    assert!(!shell.workspace.classic_layout);
                    assert_eq!(shell.workspace.new_layout_boxy, boxy);
                    assert!(shell.workspace.sidebar_visible);
                })
                .unwrap();
            window
                .update(visual, |shell, window, cx| {
                    surface.close(shell, session, window, cx)
                })
                .unwrap();
            visual.run_until_parked();
        }
    }
}

#[gpui::test]
fn new_layout_app_tabs_survive_frame_changes_and_close_like_session_tabs(cx: &mut TestAppContext) {
    cx.update(super::bind_keys);
    for boxy in [false, true] {
        let (window, session) = setup(cx, false, boxy);
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        for surface in [Surface::Inbox, Surface::Notes, Surface::Automations] {
            window
                .update(visual, |shell, window, cx| surface.open(shell, window, cx))
                .unwrap();
            visual.run_until_parked();
        }
        let order = window
            .update(visual, |shell, _, cx| {
                for app in [
                    app_views::AppView::Inbox,
                    app_views::AppView::Notes,
                    app_views::AppView::Automations,
                ] {
                    assert_eq!(
                        shell
                            .workspace
                            .title_order
                            .iter()
                            .filter(|tab| **tab == TitleTab::App(app))
                            .count(),
                        1
                    );
                }
                let order = shell.workspace.title_order.clone();
                shell.workspace.new_layout_boxy = !boxy;
                shell.layout_changed(cx);
                order
            })
            .unwrap();
        visual.run_until_parked();
        visual.refresh().unwrap();
        for app in [
            app_views::AppView::Inbox,
            app_views::AppView::Notes,
            app_views::AppView::Automations,
        ] {
            let selector = Box::leak(format!("app-title-tab-{}", app.label()).into_boxed_str());
            let tab = visual.debug_bounds(selector).unwrap();
            visual.simulate_click(tab.center(), Default::default());
            visual.run_until_parked();
            window
                .read_with(visual, |shell, _| {
                    assert_eq!(shell.active_app_view, Some(app));
                    assert_eq!(shell.workspace.title_order, order);
                })
                .unwrap();
        }
        // Close an inactive tab through its button, then the selected tabs
        // through Cmd-W, preserving the underlying conversation.
        visual.refresh().unwrap();
        let close = visual.debug_bounds("close-app-tab-Inbox").unwrap();
        visual.simulate_click(close.center(), Default::default());
        visual.run_until_parked();
        for _ in 0..2 {
            visual.simulate_keystrokes("cmd-w");
            visual.run_until_parked();
        }
        window
            .read_with(visual, |shell, cx| {
                assert!(!shell.app_view_open());
                assert_eq!(shell.workspace.active, Some(session));
                assert!(
                    shell
                        .workspace
                        .title_order
                        .iter()
                        .all(|tab| !matches!(tab, TitleTab::App(_)))
                );
                assert_eq!(shell.active_title_tab(cx), Some(TitleTab::Session(session)));
            })
            .unwrap();
    }
}

fn assert_settings_navigation(classic: bool, cx: &mut TestAppContext) {
    let (window, session) = setup(cx, classic, false);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    window
        .update(visual, |shell, window, cx| {
            shell.open_settings(crate::settings::Section::General, window, cx)
        })
        .unwrap();
    visual.run_until_parked();
    visual.refresh().unwrap();

    // The toggle steps aside but keeps its slot, so the title bar holds
    // still, and the shortcut can't hide what Settings put in the sidebar's
    // place.
    let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
    assert_eq!(toggle.size.width, px(24.));
    visual.dispatch_action(ToggleSidebar);
    visual.run_until_parked();
    window
        .read_with(visual, |shell, _| {
            assert!(shell.settings.is_some());
            assert!(shell.workspace.sidebar_visible);
            assert!(shell.workspace.project_rail_open);
        })
        .unwrap();

    visual.refresh().unwrap();
    let back = visual.debug_bounds("visit-back").unwrap();
    visual.simulate_click(back.center(), Default::default());
    visual.run_until_parked();
    window
        .read_with(visual, |shell, _| {
            assert!(shell.settings.is_none(), "Back leaves Settings");
            assert_eq!(shell.workspace.active, Some(session));
        })
        .unwrap();
}

#[gpui::test]
fn settings_back_and_sidebar_toggle_in_new_layout(cx: &mut TestAppContext) {
    assert_settings_navigation(false, cx);
}

#[gpui::test]
fn settings_back_and_sidebar_toggle_in_original_layout(cx: &mut TestAppContext) {
    assert_settings_navigation(true, cx);
}

/// Boxy and original layouts once painted the sidebar and the main column the
/// same shell color, so every theme looked alike. Each now paints the theme's
/// own pair: the sidebar's chrome wash beside the main pane's color.
#[gpui::test]
fn boxy_layouts_set_the_sidebar_apart_from_the_main_column(cx: &mut TestAppContext) {
    // One window: theme colors are app-wide, so two windows with different
    // themes would keep repainting each other's colors.
    let (window, _) = setup(cx, false, true);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    for classic in [false, true] {
        for theme in ["light", "dark"] {
            window
                .update(visual, |shell, _, cx| {
                    shell.workspace.classic_layout = classic;
                    shell.layout_changed(cx);
                    let p = &mut shell.workspace.preferences;
                    p.apply_workspace_theme("solarized".into());
                    p.theme = theme.into();
                    p.body_glass = false;
                    cx.notify();
                })
                .unwrap();
            visual.run_until_parked();
            visual.refresh().unwrap();
            let sidebar = visual.debug_bounds("sidebar-frame").unwrap();
            let main = visual.debug_bounds("main-pane-frame").unwrap();
            window
                .update(visual, |_, window, _| {
                    let scale = window.scale_factor();
                    let quads = window.painted_quads();
                    let painted_at = |color: gpui::Hsla, at: gpui::Point<Pixels>| {
                        let point = gpui::point(
                            gpui::ScaledPixels(f32::from(at.x) * scale),
                            gpui::ScaledPixels(f32::from(at.y) * scale),
                        );
                        quads.iter().any(|quad| {
                            quad.background == color.into()
                                && quad.bounds.contains(&point)
                                && quad.content_mask.bounds.contains(&point)
                        })
                    };
                    let main_color: gpui::Hsla =
                        gpui::rgb(if theme == "light" { 0xfdf6e3 } else { 0x00212b }).into();
                    assert_eq!(main_pane_fill(), main_color, "{theme} classic={classic}");
                    assert!(
                        painted_at(inset_chrome_wash(), sidebar.center()),
                        "{theme} classic={classic}: the sidebar lost its chrome wash"
                    );
                    assert!(
                        !painted_at(main_pane_fill(), sidebar.center()),
                        "{theme} classic={classic}: the sidebar paints the main color"
                    );
                    assert!(
                        painted_at(
                            main_pane_fill(),
                            main.bottom_left() + gpui::point(px(4.), px(-40.))
                        ),
                        "{theme} classic={classic}: the main column lost the theme's main color"
                    );
                })
                .unwrap();
        }
    }
}

#[test]
fn a_header_leaves_the_window_controls_only_the_room_the_panel_does_not() {
    use super::classic_surfaces::caption_room;
    assert_eq!(caption_room(138., 0.), 138.);
    // Sliding open: the header gives the room back as the panel takes it.
    assert_eq!(caption_room(138., 60.), 78.);
    assert_eq!(caption_room(138., 320.), 0.);
    assert_eq!(caption_room(0., 320.), 0.);
}

#[gpui::test]
fn the_navigation_never_covers_what_it_floats_over(cx: &mut TestAppContext) {
    // Classic with the rail closed: the navigation overhangs the sidebar's
    // header, whose title starts after the arrows.
    let (window, _) = setup(cx, true, false);
    window
        .update(cx, |shell, _, cx| {
            shell.workspace.project_rail_open = false;
            shell.layout_changed(cx);
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    visual.refresh().unwrap();
    let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
    let forward = visual.debug_bounds("visit-forward").unwrap();
    let title = visual
        .debug_bounds("classic-workspace-picker")
        .or_else(|| visual.debug_bounds("classic-workspace-title"))
        .expect("the sidebar's header names the working copy");
    assert!(toggle.right() <= forward.left());
    if !window
        .read_with(visual, |shell, cx| shell.classic_titlebar_spans_window(cx))
        .unwrap()
    {
        assert!(
            title.left() >= forward.right(),
            "the title starts after the navigation: {title:?} against {forward:?}"
        );
    }
    // Focused, sidebar shut: the tabs start after the arrows.
    for boxy in [false, true] {
        let (window, _) = setup(cx, false, boxy);
        window
            .update(cx, |shell, _, cx| {
                shell.workspace.sidebar_visible = false;
                shell.layout_changed(cx);
            })
            .unwrap();
        let visual = &mut VisualTestContext::from_window(window.into(), cx);
        visual.run_until_parked();
        visual.refresh().unwrap();
        let toggle = visual.debug_bounds("toggle-sidebar").unwrap();
        let back = visual.debug_bounds("visit-back").unwrap();
        let forward = visual.debug_bounds("visit-forward").unwrap();
        assert!(toggle.left() >= px(super::TRAFFIC_LIGHTS) || cfg!(target_os = "macos"));
        assert!(toggle.right() <= back.left() && back.right() <= forward.left());
    }
}
