//! «New Captain», after Capy's: the face the new personal agent will wear,
//! its name, the role it starts from (or a blank one) and Create. The roles
//! menu it replaces made the bot at once and left naming for later.
use super::*;
use crate::input::Input;
use monocode_core::mono::bots;

pub(super) struct NewCaptain {
    role: &'static str,
    name: Entity<Input>,
    size: gpui::Size<gpui::Pixels>,
}

impl Monos {
    pub(super) fn open_new_captain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let role = bots::offered().next().map_or("custom", |role| role.role);
        let name = cx.new(|cx| {
            let mut input = Input::field("Name", "Name your Captain", content(0.35), cx);
            input.set_text(role_name(role).into(), cx);
            input
        });
        window.focus(&name.focus_handle(cx), cx);
        self.roles_menu = None;
        self.new_captain = Some(NewCaptain {
            role,
            name,
            size: window.viewport_size(),
        });
        cx.notify();
    }

    fn pick_captain_role(&mut self, role: &'static str, cx: &mut Context<Self>) {
        let Some(draft) = self.new_captain.as_mut() else {
            return;
        };
        // A name still the old role's (or none) follows the role; one the
        // user typed stays.
        let previous = std::mem::replace(&mut draft.role, role);
        let name = draft.name.clone();
        let typed = name.read(cx).content.trim().to_owned();
        if typed.is_empty() || typed == role_name(previous) {
            name.update(cx, |input, cx| input.set_text(role_name(role).into(), cx));
        }
        cx.notify();
    }

    fn create_captain(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.new_captain.take() else {
            return;
        };
        let name = draft.name.read(cx).content.clone();
        let Some(id) = self
            .roster
            .create_captain(&self.root, draft.role, &name)
            .map(|mono| mono.id.clone())
        else {
            return;
        };
        self.save(cx);
        self.open(&id, window, cx);
        self.changed(cx);
    }

    pub(super) fn new_captain_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let draft = self.new_captain.as_ref()?;
        let (mascot, color) = self.roster.next_look();
        // The face the Captain will wear: its role's persona on the next look.
        let preview = Mono {
            mascot,
            color,
            kind: Some("bot".into()),
            bot_role: Some(draft.role.into()),
            ..Default::default()
        };
        let roles: Vec<(&'static str, &'static str)> = bots::offered()
            .map(|role| (role.role, role.name))
            .chain(std::iter::once(("custom", "Blank")))
            .collect();
        let note = bots::role(draft.role)
            .filter(|_| draft.role != "custom")
            .map_or(
                "Start from scratch: you write its soul and pick its model.",
                |role| role.description,
            );
        let card = div()
            .id("new-captain")
            .debug_selector(|| "new-captain".into())
            .role(Role::Dialog)
            .aria_label("New Captain")
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px_5()
                    .pt_4()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child("New Captain")
                    .child(icon_button("new-captain-close", "Close", "close").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.new_captain = None;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .px_5()
                    .pt_4()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_3()
                    .child(avatar::avatar(&preview.face(), color, "idle", 88., true))
                    .child(
                        div()
                            .debug_selector(|| "new-captain-name".into())
                            .w(px(280.))
                            .px_3()
                            .py_1p5()
                            .rounded(px(8.))
                            .bg(content(0.05))
                            .text_size(px(20.))
                            .text_color(content(1.))
                            .child(draft.name.clone()),
                    )
                    .child(
                        div()
                            .w_full()
                            .mt_2()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.5))
                                    .child("Starts as"),
                            )
                            .child(
                                div()
                                    .id("new-captain-roles")
                                    .role(Role::RadioGroup)
                                    .aria_label("Starts as")
                                    .flex()
                                    .flex_wrap()
                                    .gap(px(6.))
                                    .children(roles.into_iter().map(|(role, label)| {
                                        let chosen = draft.role == role;
                                        div()
                                            .id(SharedString::from(format!(
                                                "new-captain-role-{role}"
                                            )))
                                            .debug_selector(move || {
                                                format!("new-captain-role-{role}")
                                            })
                                            .role(Role::RadioButton)
                                            .aria_label(label)
                                            .aria_selected(chosen)
                                            .px(px(10.))
                                            .py(px(5.))
                                            .rounded(px(8.))
                                            .text_size(px(12.5))
                                            .cursor_pointer()
                                            .map(|d| {
                                                if chosen {
                                                    d.bg(content(0.12)).text_color(content(1.))
                                                } else {
                                                    d.bg(content(0.04))
                                                        .text_color(content(0.65))
                                                        .hover(|s| s.text_color(content(1.)))
                                                }
                                            })
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.pick_captain_role(role, cx)
                                            }))
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .line_height(px(18.))
                                    .text_color(content(0.55))
                                    .child(note),
                            ),
                    ),
            )
            .child(
                div()
                    .px_5()
                    .py_4()
                    .border_t_1()
                    .border_color(content(0.07))
                    .flex()
                    .justify_end()
                    .child(
                        crate::modal::primary_button("new-captain-create", "Create Captain")
                            .debug_selector(|| "new-captain-create".into())
                            .on_click(cx.listener(|this, _, w, cx| this.create_captain(w, cx))),
                    ),
            )
            .into_any_element();
        let weak = cx.weak_entity();
        Some(self.centered_dialog(
            "new-captain-dialog",
            draft.size,
            (520., 560.),
            None,
            card,
            move |_, cx| {
                let _ = weak.update(cx, |this, cx| {
                    this.new_captain = None;
                    cx.notify();
                });
            },
        ))
    }
}

/// The name a Captain of this role starts with.
fn role_name(role: &str) -> &'static str {
    bots::role(role)
        .filter(|_| role != "custom")
        .map_or("New Captain", |role| role.name)
}
