//! The Email tab's rules (Mailflare's routing on the user's own Gmail) and what the agents did
//! with the mail. A rule says which new mail it is for (a sender, words of the subject) and
//! what happens to it: a Gmail label, a star, read, archived, handed to an agent. Only the user
//! makes or changes rules, here.
use super::connect::Agent;
use super::*;

pub(super) struct RuleForm {
    name: Entity<Input>,
    pub(super) from: Entity<Input>,
    subject: Entity<Input>,
    pub(super) label: Entity<Input>,
    star: bool,
    read: bool,
    archive: bool,
    agent: String,
}

impl RuleForm {
    fn new(cx: &mut Context<Mailbox>) -> Self {
        let field = |label: &str, placeholder: &str, cx: &mut Context<Mailbox>| {
            let (label, placeholder) = (label.to_owned(), placeholder.to_owned());
            cx.new(move |cx| Input::field(label, placeholder, content(0.35), cx))
        };
        Self {
            name: field("Name", "Banks", cx),
            from: field("From contains", "bcp, yape, interbank", cx),
            subject: field("Subject contains", "consumo, transferencia", cx),
            label: field("Gmail label", "Finanzas", cx),
            star: false,
            read: false,
            archive: false,
            agent: String::new(),
        }
    }

    fn rule(&self, cx: &App) -> model::Rule {
        let read = |input: &Entity<Input>| input.read(cx).content.trim().to_owned();
        model::Rule {
            name: read(&self.name),
            from: read(&self.from),
            subject: read(&self.subject),
            label: read(&self.label),
            star: self.star,
            read: self.read,
            archive: self.archive,
            agent: self.agent.clone(),
            enabled: true,
            ..model::Rule::default()
        }
    }
}

fn section(title: &'static str, note: &'static str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(content(1.))
                .child(title),
        )
        .child(
            div()
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(content(0.55))
                .child(note),
        )
}

fn toggle(id: String, label: impl Into<SharedString>, on: bool) -> Stateful<Div> {
    let label: SharedString = label.into();
    let selector = id.clone();
    div()
        .id(SharedString::from(id))
        .debug_selector(move || selector.clone())
        .role(Role::Switch)
        .aria_label(label.clone())
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .px(px(10.))
        .py(px(5.))
        .text_size(px(12.))
        .cursor_pointer()
        .text_color(content(if on { 1. } else { 0.6 }))
        .when(on, |d| d.bg(content(0.1)))
        .when(!on, |d| d.hover(|s| s.bg(content(0.06))))
        .child(label)
}

fn agent_name(agents: &[Agent], id: &str) -> String {
    agents
        .iter()
        .find(|a| a.id == id)
        .map(|a| a.label.clone())
        .unwrap_or_else(|| "an agent no longer here".into())
}

/// «Label Finanzas · star · → Finance»: what a rule does, in a line.
fn does(rule: &model::Rule, agents: &[Agent]) -> String {
    let mut parts = Vec::new();
    if !rule.label.is_empty() {
        parts.push(format!("label «{}»", rule.label));
    }
    for (on, word) in [
        (rule.star, "star"),
        (rule.read, "mark read"),
        (rule.archive, "archive"),
    ] {
        if on {
            parts.push(word.to_owned());
        }
    }
    if !rule.agent.is_empty() {
        parts.push(format!("→ {}", agent_name(agents, &rule.agent)));
    }
    parts.join(" · ")
}

impl Mailbox {
    fn reload_rules(&mut self, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { (service.rules(), service.activity(40)) });
        cx.spawn(async move |this, cx| {
            let (rules, activity) = task.await;
            let _ = this.update(cx, |this, cx| {
                this.rules = rules;
                this.activity = activity;
                cx.notify();
            });
        })
        .detach();
    }

    fn save_rule(&mut self, rule: model::Rule, close: bool, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.save_rule(rule) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(_) => {
                        this.rule_error = None;
                        if close {
                            this.rule_form = None;
                        }
                        this.reload_rules(cx);
                    }
                    Err(error) => this.rule_error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_rule(&mut self, id: String, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.remove_rule(&id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.rule_error = Some(error);
                }
                this.reload_rules(cx);
            });
        })
        .detach();
    }

    /// The rules, each with its switch and a way to remove it, and the form for a new one.
    pub(super) fn rules_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let agents = self.agents.clone();
        div()
            .id("mail-rules")
            .debug_selector(|| "mail-rules".into())
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(section(
                "Rules",
                "For new mail only. Label it in Gmail, star it, mark it read, archive it, or hand it to an agent, such as the bank's notices to Finance. Nothing is deleted or sent.",
            ))
            .children(self.rules.iter().map(|rule| {
                let (id, switch_id) = (rule.id.clone(), rule.id.clone());
                let flipped = model::Rule {
                    enabled: !rule.enabled,
                    ..rule.clone()
                };
                let when = [
                    (!rule.from.is_empty()).then(|| format!("from {}", rule.from)),
                    (!rule.subject.is_empty()).then(|| format!("subject has {}", rule.subject)),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" and ");
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(8.))
                    .border_1()
                    .border_color(content(0.1))
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(13.))
                                    .text_color(content(if rule.enabled { 1. } else { 0.5 }))
                                    .child(if rule.name.is_empty() {
                                        "Rule".to_owned()
                                    } else {
                                        rule.name.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(content(0.55))
                                    .child(format!("When {when}: {}", does(rule, &agents))),
                            ),
                    )
                    .child(
                        toggle(
                            format!("mail-rule-on-{switch_id}"),
                            if rule.enabled { "On" } else { "Off" },
                            rule.enabled,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.save_rule(flipped.clone(), false, cx)
                        })),
                    )
                    .child(
                        tool(
                            SharedString::from(format!("mail-rule-remove-{id}")),
                            "Remove rule",
                            "trash",
                            content(0.5),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| this.remove_rule(id.clone(), cx))),
                    )
            }))
            .child(match &self.rule_form {
                None => div()
                    .child(
                        text_button("mail-rule-add", "Add rule", 0.8)
                            .debug_selector(|| "mail-rule-add".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rule_form = Some(RuleForm::new(cx));
                                cx.notify();
                            })),
                    )
                    .into_any_element(),
                Some(form) => self.rule_form_view(form, &agents, window, cx),
            })
            .when_some(self.rule_error.clone(), |d, error| d.child(alert(error)))
            .into_any_element()
    }

    fn rule_form_view(
        &self,
        form: &RuleForm,
        agents: &[Agent],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let field = |label: &'static str, input: &Entity<Input>, window: &Window, cx: &App| {
            let focused = input.focus_handle(cx).is_focused(window);
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(content(0.7))
                        .child(label),
                )
                .child(
                    div()
                        .h(px(32.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .border_1()
                        .border_color(content(if focused { 0.25 } else { 0.1 }))
                        .bg(background_base())
                        .child(div().w_full().child(input.clone())),
                )
        };
        let flip = |pick: fn(&mut RuleForm)| {
            move |this: &mut Mailbox,
                  _: &gpui::ClickEvent,
                  _: &mut Window,
                  cx: &mut Context<Mailbox>| {
                if let Some(form) = this.rule_form.as_mut() {
                    pick(form);
                }
                cx.notify();
            }
        };
        div()
            .id("mail-rule-form")
            .flex()
            .flex_col()
            .gap(px(10.))
            .p(px(12.))
            .border_1()
            .border_color(content(0.12))
            .child(field("Name", &form.name, window, cx))
            .child(field(
                "From contains (any of, by commas)",
                &form.from,
                window,
                cx,
            ))
            .child(field(
                "Subject contains (any of, by commas)",
                &form.subject,
                window,
                cx,
            ))
            .child(field("Gmail label (optional)", &form.label, window, cx))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .child(
                        toggle("mail-rule-star".into(), "Star", form.star)
                            .on_click(cx.listener(flip(|f| f.star = !f.star))),
                    )
                    .child(
                        toggle("mail-rule-read".into(), "Mark read", form.read)
                            .on_click(cx.listener(flip(|f| f.read = !f.read))),
                    )
                    .child(
                        toggle("mail-rule-archive".into(), "Archive", form.archive)
                            .on_click(cx.listener(flip(|f| f.archive = !f.archive))),
                    ),
            )
            .when(!agents.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(content(0.7))
                                .child("Hand it to"),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap(px(4.))
                                .child(
                                    toggle(
                                        "mail-rule-agent-none".into(),
                                        "No agent",
                                        form.agent.is_empty(),
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            if let Some(form) = this.rule_form.as_mut() {
                                                form.agent.clear();
                                            }
                                            cx.notify();
                                        },
                                    )),
                                )
                                .children(agents.iter().map(|agent| {
                                    let id = agent.id.clone();
                                    toggle(
                                        format!("mail-rule-agent-{}", agent.id),
                                        agent.label.clone(),
                                        form.agent == agent.id,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            if let Some(form) = this.rule_form.as_mut() {
                                                form.agent = id.clone();
                                            }
                                            cx.notify();
                                        },
                                    ))
                                })),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        primary("mail-rule-save", "Save rule", true)
                            .debug_selector(|| "mail-rule-save".into())
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(form) = this.rule_form.as_ref() {
                                    let rule = form.rule(cx);
                                    this.save_rule(rule, true, cx);
                                }
                            })),
                    )
                    .child(
                        text_button("mail-rule-cancel", "Cancel", 0.6).on_click(cx.listener(
                            |this, _, _, cx| {
                                this.rule_form = None;
                                this.rule_error = None;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .into_any_element()
    }

    /// What the agents did with the mail lately, newest first: who, what and whether it worked.
    pub(super) fn activity_view(&self) -> AnyElement {
        let now = model::now_ms();
        div()
            .id("mail-activity")
            .debug_selector(|| "mail-activity".into())
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(section(
                "Agent activity",
                "What each agent read, searched, drafted or proposed. It never keeps the mail itself.",
            ))
            .when(self.activity.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(12.))
                        .text_color(content(0.45))
                        .child("No agent has used your mail yet."),
                )
            })
            .children(self.activity.iter().map(|entry| {
                let who = if entry.agent.is_empty() {
                    "An agent".to_owned()
                } else {
                    agent_name(&self.agents, &entry.agent)
                };
                div()
                    .flex()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .w(px(64.))
                            .flex_shrink_0()
                            .text_color(content(0.4))
                            .child(model::row_time(entry.at, now)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .text_color(content(if entry.ok { 0.8 } else { 0.5 }))
                            .child(format!(
                                "{who} · {}{}{}",
                                entry.action.trim_start_matches("mail."),
                                if entry.about.is_empty() { String::new() } else { format!(" · {}", entry.about) },
                                if entry.ok { "" } else { " · failed" }
                            )),
                    )
            }))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_says_what_it_does_in_a_line() {
        let agents = vec![Agent {
            id: "fin".into(),
            label: "Finance".into(),
        }];
        let rule = model::Rule {
            label: "Finanzas".into(),
            star: true,
            agent: "fin".into(),
            ..model::Rule::default()
        };
        assert_eq!(does(&rule, &agents), "label «Finanzas» · star · → Finance");
        assert_eq!(agent_name(&agents, "gone"), "an agent no longer here");
    }
}
