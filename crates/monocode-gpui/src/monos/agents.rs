//! The Agents zone (PersonalRail.tsx): the Principal pinned on top, then the
//! bots and the groups, each under its own label. The new-bot menu offers a
//! blank bot and every role in BOT_ROLES; MonoIntroCard invites the first
//! Mono, and "A bot of your own" the first bot made from scratch.
use super::*;
use gpui::{MouseDownEvent, Pixels, Point};
use monocode_core::mono::bots;

/// Which Monos a rail section lists.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    /// Coding Monos, under Code.
    Coding,
    /// The Principal bot, pinned at the top of Agents.
    Principal,
    Bots,
    Groups,
}

impl Section {
    pub(super) fn holds(self, mono: &Mono) -> bool {
        match self {
            Self::Coding => mono.is_coding(),
            Self::Principal => mono.is_principal(),
            Self::Bots => mono.is_bot() && !mono.is_principal(),
            Self::Groups => mono.is_group(),
        }
    }
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Coding => "Monos",
            Self::Principal => "Principal",
            Self::Bots => "Bots",
            Self::Groups => "Groups",
        }
    }
}

/// `RailDivider`: a hairline between the rail's groups of items.
fn divider() -> gpui::Div {
    div()
        .mx_3()
        .mb_2()
        .h(px(1.))
        .flex_shrink_0()
        .bg(content(0.1))
}

impl Monos {
    /// The whole Agents zone, for the sidebar in either layout.
    pub(crate) fn agents_rail(&mut self, original: bool, cx: &mut Context<Self>) -> AnyElement {
        self.rail_ids.clear();
        let has_principal = self.roster.monos.iter().any(Mono::is_principal);
        let principal = if has_principal {
            self.rail_impl(None, original, false, Section::Principal, cx)
        } else {
            self.principal_row(cx)
        };
        let bots = self.rail_impl(None, original, false, Section::Bots, cx);
        let has_groups = self.roster.monos.iter().any(Mono::is_group);
        let groups = has_groups.then(|| self.rail_impl(None, original, false, Section::Groups, cx));
        let custom = self
            .roster
            .monos
            .iter()
            .any(|m| m.is_bot() && m.bot_role.as_deref() == Some("custom"));
        let intro = (!custom && !self.roster.bot_intro_dismissed).then(|| self.bot_intro_card(cx));
        div()
            .id("agents-rail")
            .debug_selector(|| "agents-rail".into())
            .flex()
            .flex_col()
            .child(div().px_2().pb_2().child(principal))
            .child(divider())
            .child(bots)
            .children(intro.map(|card| div().px_2().pb_3().child(card)))
            .when_some(groups, |d, groups| d.child(divider()).child(groups))
            .children(self.roles_surface(cx))
            .into_any_element()
    }

    /// With no Principal yet, its row makes one, as PersonalRail does.
    fn principal_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("agents-principal-new")
            .role(Role::Button)
            .aria_label("Principal")
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .opacity(0.65)
            .hover(|s| s.bg(content(0.06)).opacity(1.))
            .child(icon("account-user", 16.).text_color(content(0.6)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(14.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child("Principal"),
            )
            .on_click(cx.listener(|this, _, w, cx| this.create_bot("principal", false, w, cx)))
            .into_any_element()
    }

    /// The Bots label's "+": opens the new-bot menu at the pointer.
    pub(super) fn new_bot_button(&mut self, cx: &mut Context<Self>) -> AnyElement {
        icon_button("new-bot", "New bot", "plus")
            .debug_selector(|| "sidebar-new-bot".into())
            .size(px(20.))
            .mr(px(-3.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.roles_menu = Some(event.position);
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// With no bot yet, a "New bot" row opens the same menu.
    pub(super) fn new_bot_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("new-bot-row")
            .debug_selector(|| "new-bot".into())
            .role(Role::Button)
            .aria_label("New bot")
            .h(px(32.))
            .px_2()
            .flex()
            .items_center()
            .gap_2()
            .text_size(px(14.))
            .text_color(content(0.5))
            .hover(|s| s.bg(content(0.1)).text_color(content(1.)))
            .child(icon("plus", 16.))
            .child("New bot")
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.roles_menu = Some(event.position);
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    /// The new-bot menu: a blank bot, then every offered role.
    fn roles_surface(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let position: Point<Pixels> = self.roles_menu?;
        let item = |id: &'static str,
                    name: &'static str,
                    note: &'static str,
                    experimental: bool,
                    blank: bool,
                    cx: &mut Context<Self>| {
            div()
                .id(SharedString::from(format!("new-bot-{id}")))
                .debug_selector(move || format!("new-bot-{id}"))
                .role(Role::MenuItem)
                .aria_label(name)
                .w_full()
                .px(px(10.))
                .py(px(6.))
                .flex()
                .flex_col()
                .items_start()
                .hover(|s| s.bg(content(0.08)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(13.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(name)
                        .when(experimental, |d| {
                            d.child(
                                div()
                                    .px_1()
                                    .text_size(px(10.))
                                    .line_height(px(16.))
                                    .bg(accent_color().opacity(0.1))
                                    .text_color(accent_color())
                                    .child("Experimental"),
                            )
                        }),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .child(note),
                )
                .on_click(cx.listener(move |this, _, w, cx| {
                    this.roles_menu = None;
                    this.create_bot(id, blank, w, cx);
                }))
        };
        let mut panel = div()
            .w(px(260.))
            .p_1()
            .flex()
            .flex_col()
            .child(item(
                "custom",
                "Blank bot",
                "Start from scratch: name, soul, model",
                false,
                true,
                cx,
            ))
            .child(div().my_1().h(px(1.)).bg(content(0.1)));
        for role in bots::offered() {
            panel = panel.child(item(
                role.role,
                role.name,
                role.description,
                role.experimental,
                false,
                cx,
            ));
        }
        let weak = cx.weak_entity();
        Some(
            popover("new-bot-menu", Anchor::Point(position))
                .without_animation()
                .side(crate::popover::Side::Right)
                .priority(90)
                .child(panel)
                .on_dismiss(move |_, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        this.roles_menu = None;
                        cx.notify();
                    });
                })
                .into_any_element(),
        )
    }

    /// MonoIntroCard: the next Mono's mascot, what Monos are, and Create mono.
    pub(super) fn mono_intro_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (mascot, color) = self.roster.next_look();
        intro_card(
            "Meet Monos",
            "Agents of your own that work across your projects and run habits on their own.",
            "Create mono",
            &mascot,
            color,
        )
        .child_create(cx.listener(|this, _, w, cx| this.create(Vec::new(), w, cx)))
    }

    /// "A bot of your own", until there is a bot made from scratch or it is closed.
    fn bot_intro_card(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let (mascot, color) = self.roster.next_look();
        intro_card(
            "A bot of your own",
            "Beyond Assistant, Finance, Documents and Research: give it a name, write its soul and pick its tools.",
            "Create bot",
            &mascot,
            color,
        )
        .close(cx.listener(|this, _, _, cx| {
            this.roster.bot_intro_dismissed = true;
            this.save(cx);
            cx.notify();
        }))
        .child_create(cx.listener(|this, _, w, cx| this.create_bot("custom", true, w, cx)))
    }

    /// Makes a bot of this role and opens it on its settings, as a new Mono.
    pub(crate) fn create_bot(
        &mut self,
        role: &str,
        blank: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self
            .roster
            .create_bot(&self.root, role, blank)
            .map(|mono| mono.id.clone())
        else {
            return;
        };
        self.save(cx);
        self.open(&id, window, cx);
        self.settings = Some(SettingsSection::General);
        window.focus(&self.settings_focus, cx);
        self.changed(cx);
    }
}

type Click = Box<dyn Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static>;

/// MonoIntroCard's frame, built before its buttons are wired.
struct IntroCard {
    title: &'static str,
    body: &'static str,
    create: &'static str,
    mascot: String,
    color: u32,
    close: Option<Click>,
}

fn intro_card(
    title: &'static str,
    body: &'static str,
    create: &'static str,
    mascot: &str,
    color: u32,
) -> IntroCard {
    IntroCard {
        title,
        body,
        create,
        mascot: mascot.into(),
        color,
        close: None,
    }
}

impl IntroCard {
    fn close(
        mut self,
        on_close: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.close = Some(Box::new(on_close));
        self
    }
    fn child_create(
        self,
        on_create: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    ) -> AnyElement {
        let mut ink = floating_fill();
        ink.a = 1.;
        div()
            .id(SharedString::from(format!("intro-{}", self.title)))
            .debug_selector({
                let title = self.title;
                move || format!("intro-card-{title}")
            })
            .role(Role::Group)
            .aria_label(self.title)
            .overflow_hidden()
            .bg(content(0.03))
            .child(
                div()
                    .pl(px(10.))
                    .pr_2()
                    .pt_2()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child(self.title),
                    )
                    .when_some(self.close, |d, on_close| {
                        d.child(
                            div()
                                .id("intro-dismiss")
                                .role(Role::Button)
                                .aria_label("Dismiss")
                                .size(px(20.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_color(content(0.45))
                                .hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                                .child(icon("close", 12.))
                                .on_click(on_close),
                        )
                    }),
            )
            .child(
                div()
                    .h(px(48.))
                    .flex()
                    .items_end()
                    .justify_center()
                    .pb_1()
                    .child(avatar::avatar(&self.mascot, self.color, "idle", 32., false)),
            )
            .child(
                div()
                    .px(px(10.))
                    .pt(px(6.))
                    .text_size(px(11.))
                    .line_height(px(15.))
                    .text_color(content(0.55))
                    .child(self.body),
            )
            .child(
                div().p_2().child(
                    div()
                        .id("intro-create")
                        .role(Role::Button)
                        .aria_label(self.create)
                        .h(px(28.))
                        .w_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .bg(content(1.))
                        .text_color(ink)
                        .text_size(px(12.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .hover(|s| s.bg(content(0.85)))
                        .child(self.create)
                        .on_click(on_create),
                ),
            )
            .into_any_element()
    }
}
