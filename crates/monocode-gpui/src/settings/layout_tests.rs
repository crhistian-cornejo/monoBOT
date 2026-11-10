use super::*;
use gpui::{
    Bounds, TestAppContext, VisualTestContext, WindowBounds, WindowHandle, WindowOptions, point,
    size,
};

fn window(section: Section, cx: &mut TestAppContext) -> WindowHandle<SettingsView> {
    let window = cx.update(|cx| {
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                    point(px(0.), px(0.)),
                    size(px(1200.), px(800.)),
                ))),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|cx| {
                    let mut view = SettingsView::new(
                        section,
                        Preferences::default(),
                        SettingsContext::default(),
                        cx,
                    );
                    view.discovery_task = None;
                    view.discovery_loading = false;
                    view
                })
            },
        )
        .unwrap()
    });
    cx.refresh().unwrap();
    cx.run_until_parked();
    window
}

#[gpui::test]
fn workspace_style_is_never_offered(cx: &mut TestAppContext) {
    // Tauri has one square workspace frame, so there is nothing to choose.
    let window = window(Section::Appearance, cx);
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    for classic in [false, true] {
        window
            .update(visual, |view, _, cx| {
                view.ctx.classic = classic;
                cx.notify();
            })
            .unwrap();
        visual.run_until_parked();
        assert!(
            visual
                .debug_bounds("settings-new-layout-style-boxy")
                .is_none()
        );
        assert!(
            visual
                .debug_bounds("settings-new-layout-style-rounded")
                .is_none()
        );
    }
}

#[gpui::test]
fn skills_document_has_its_own_full_height_pane_and_compact_actions(cx: &mut TestAppContext) {
    let window = window(Section::Skills, cx);
    window
        .update(cx, |view, _, cx| {
            let path = std::path::PathBuf::from("/project/.agents/skills/review/SKILL.md");
            view.discovery.skills = vec![data::Skill {
                name: "review".into(),
                description: "Review changes".into(),
                path: path.clone(),
                scope: "project".into(),
                provider: "agents".into(),
            }];
            view.skill_preview = Some(path);
            view.skill_preview_text = Some(Ok(skill_document::DocumentData::new(format!(
                "---\nname: review\n---\n# Review\n\n{}",
                "Instructions.\n\n".repeat(150)
            ))));
            cx.notify();
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let left = visual.debug_bounds("skills-list-scroll").unwrap();
    let right = visual.debug_bounds("skill-preview-pane").unwrap();
    let body = visual.debug_bounds("skill-document").unwrap();
    assert_eq!(left.size.height, px(800.));
    assert_eq!(right.size.height, px(800.));
    assert!(right.left() >= left.right());
    assert!(body.bottom() <= right.bottom());
    assert!(body.size.height > px(600.));
    assert!(visual.debug_bounds("transcript").is_none());
    for id in [
        "skill-preview-action",
        "skill-copy-action",
        "skill-reveal-action",
    ] {
        assert_eq!(
            visual.debug_bounds(id).unwrap().size,
            size(px(20.), px(20.))
        );
    }
    let source = visual
        .debug_bounds("settings-skill-document-source")
        .unwrap();
    assert!(source.size.width < px(100.));
    visual.simulate_click(source.center(), Default::default());
    visual.run_until_parked();
    window
        .update(visual, |view, _, _| {
            assert_eq!(view.prefs.skill_source_paths.len(), 1)
        })
        .unwrap();
    visual.simulate_resize(size(px(700.), px(800.)));
    visual.run_until_parked();
    // Deliver the frame that applies the measured Skills container breakpoint.
    visual.update(|window, cx| {
        window.simulate_next_frame(cx);
    });
    visual.run_until_parked();
    let left = visual.debug_bounds("skills-list-scroll").unwrap();
    let right = visual.debug_bounds("skill-preview-pane").unwrap();
    assert!(
        right.top() >= left.bottom(),
        "list={left:?}, preview={right:?}"
    );
    assert_eq!(left.size.width, px(700.));
    assert_eq!(right.size.width, px(700.));
    assert!(visual.debug_bounds("skill-document").unwrap().size.height > px(200.));
}

#[gpui::test]
fn mono_mascot_is_beside_name_and_sidebar_switch_stays_visible(cx: &mut TestAppContext) {
    let window = window(Section::Monos, cx);
    window
        .update(cx, |view, _, cx| {
            view.ctx.monos = vec![MonoSetting {
                id: "layout-mono".into(),
                name: "Big Boss".into(),
                mascot: "ghost".into(),
                color: 0xee99bb,
                projects: vec!["monocode".into(), "mono-web".into()],
                show: false,
                folders: false,
            }];
            cx.notify();
        })
        .unwrap();
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    visual.run_until_parked();
    let mascot = visual.debug_bounds("settings-mono-mascot").unwrap();
    let switch = visual
        .debug_bounds("settings-toggle-mono-sidebar-show")
        .unwrap();
    let reset = visual.debug_bounds("settings-mono-reset").unwrap();
    assert_eq!(mascot.size, size(px(16.), px(16.)));
    assert_eq!(switch.size, size(px(36.), px(20.)));
    let name = visual.debug_bounds("settings-mono-name").unwrap();
    assert!(mascot.right() < name.left());
    assert!(mascot.left() < px(200.));
    assert!(switch.left() > px(800.));
    assert!(switch.right() <= reset.left());
    assert!(reset.right() <= px(1200.));
    assert!((mascot.top() - switch.top()).abs() < px(4.));
    assert!(
        visual
            .debug_bounds("settings-toggle-mono-sidebar-folders")
            .is_none()
    );
    visual.simulate_click(reset.center(), Default::default());
    visual.run_until_parked();
    assert!(visual.debug_bounds("settings-management-dialog").is_some());
    window
        .update(visual, |view, window, cx| {
            view.close_management(window, cx);
            cx.notify();
        })
        .unwrap();
    visual.simulate_resize(size(px(700.), px(800.)));
    visual.run_until_parked();
    let switch = visual
        .debug_bounds("settings-toggle-mono-sidebar-show")
        .unwrap();
    assert_eq!(switch.size, size(px(36.), px(20.)));
    assert!(switch.right() <= px(700.));
}

/// Each workspace theme is tried on as the pointer reaches it in the list, and the saved one comes back when the list closes unchosen.
#[gpui::test]
fn a_workspace_theme_is_tried_on_while_its_list_is_walked(cx: &mut TestAppContext) {
    use std::{cell::RefCell, rc::Rc};
    let window = window(Section::Appearance, cx);
    cx.update(|cx| {
        crate::input::bind(cx);
        crate::popover::bind(cx);
        cx.set_reduce_motion(true);
    });
    let entity = window.update(cx, |_, _, cx| cx.entity()).unwrap();
    let previews: Rc<RefCell<Vec<Option<String>>>> = Default::default();
    let chosen: Rc<RefCell<Vec<String>>> = Default::default();
    let _events = cx.update(|cx| {
        let (previews, chosen) = (previews.clone(), chosen.clone());
        cx.subscribe(&entity, move |_, event: &SettingsEvent, _| match event {
            SettingsEvent::PreviewTheme(theme) => previews.borrow_mut().push(theme.clone()),
            SettingsEvent::Changed(prefs) => {
                chosen.borrow_mut().push(prefs.workspace_theme.clone())
            }
            _ => {}
        })
    });
    let visual = &mut VisualTestContext::from_window(window.into(), cx);
    let ids: Vec<&str> = PALETTES.iter().map(|palette| palette.0).collect();
    let open = |visual: &mut VisualTestContext| {
        let select = visual
            .debug_bounds("settings-select-workspace-theme")
            .expect("the workspace theme select");
        visual.simulate_click(select.center(), Default::default());
        visual.run_until_parked();
    };
    let point_at = |visual: &mut VisualTestContext, index: usize| {
        let option = visual
            .debug_bounds(Box::leak(
                format!("settings-option-workspace-theme-{index}").into_boxed_str(),
            ))
            .expect("the option is drawn");
        visual.simulate_mouse_move(option.center(), None, Default::default());
        visual.run_until_parked();
    };
    open(visual);
    assert!(previews.borrow().is_empty(), "opening tries nothing on");
    // The pointer reaches the second theme, then the third.
    point_at(visual, 1);
    point_at(visual, 2);
    assert_eq!(
        *previews.borrow(),
        vec![Some(ids[1].to_owned()), Some(ids[2].to_owned())]
    );
    // Moving within the same row tries nothing on again.
    point_at(visual, 2);
    assert_eq!(previews.borrow().len(), 2);
    // Escape closes it unchosen: the saved theme comes back, nothing saved.
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(previews.borrow().last(), Some(&None));
    assert!(chosen.borrow().is_empty());
    // A click chooses: that is the one change saved, and the trying on ends.
    open(visual);
    point_at(visual, 2);
    let option = visual
        .debug_bounds("settings-option-workspace-theme-2")
        .unwrap();
    visual.simulate_click(option.center(), Default::default());
    visual.run_until_parked();
    assert_eq!(*chosen.borrow(), vec![ids[2].to_owned()]);
    assert_eq!(previews.borrow().last(), Some(&None));
}
