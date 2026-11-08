//! The card an agent's `mail.propose` leaves in its chat: a reply, or a new message, with
//! the account it goes from, the people it goes to, its subject and its text. It leaves only
//! when the user presses Send; Save as draft puts it in the account's Drafts and Discard drops
//! it. What happened stays on the card.
use super::*;
use gpui::{Div, Stateful};
use monocode_core::inbox::mail::{self as model, Outgoing, Person};
use serde_json::{Value, json};
use std::path::Path;

/// The card's own state: none while it waits for the user.
fn state(value: &Value) -> &str {
    value["state"].as_str().unwrap_or_default()
}

fn people(value: &Value) -> Vec<Person> {
    value
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|p| {
                    Some(Person {
                        name: p["name"].as_str().unwrap_or_default().to_owned(),
                        address: p["address"].as_str()?.to_owned(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn names(list: &[Person]) -> String {
    list.iter()
        .map(|p| model::person_label(&p.name, &p.address))
        .collect::<Vec<_>>()
        .join(", ")
}

fn line(label: &'static str, text: String) -> Div {
    div()
        .flex()
        .gap_2()
        .text_size(px(12.))
        .child(
            div()
                .w(px(52.))
                .flex_shrink_0()
                .text_color(content(0.45))
                .child(label),
        )
        .child(
            div()
                .min_w_0()
                .flex_1()
                .text_color(content(0.8))
                .child(text),
        )
}

fn action(id: String, label: &'static str, primary: bool) -> Stateful<Div> {
    let button =
        panels::button(SharedString::from(id.clone()), label).debug_selector(move || id.clone());
    if !primary {
        return button.bg(content(0.06));
    }
    let (fill, ink) = primary_action(true).unwrap_or_else(|| {
        let mut ink = floating_fill();
        ink.a = 1.;
        (content(0.92), ink)
    });
    button.bg(fill).text_color(ink)
}

pub(super) fn render(
    value: &Value,
    cx: &mut Context<crate::transcript::TranscriptView>,
) -> AnyElement {
    let id = value["id"].as_str().unwrap_or_default().to_owned();
    let to = people(&value["to"]);
    let cc = people(&value["cc"]);
    let reply = value["mode"] == "reply";
    let title = if reply {
        format!("Reply to {}", names(&to))
    } else {
        format!("New message to {}", names(&to))
    };
    let text = value["text"].as_str().unwrap_or_default().to_owned();
    let state = state(value).to_owned();
    let waiting = state.is_empty() || state == "error";
    let send = |action_name: &'static str| {
        let card = value.clone();
        move |_: &mut crate::transcript::TranscriptView,
              _: &gpui::ClickEvent,
              _: &mut Window,
              cx: &mut Context<crate::transcript::TranscriptView>| {
            cx.emit(cards::CardAction(
                json!({"type": "mail", "do": action_name, "card": card}),
            ))
        }
    };
    div()
        .id(SharedString::from(format!("mono-mail-{id}")))
        .debug_selector(|| "mono-mail-card".into())
        .rounded_lg()
        .border_1()
        .border_color(content(0.12))
        .bg(content(0.035))
        .p_3()
        .max_w(px(520.))
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(icon("mail", 16.))
                .child(div().min_w_0().truncate().child(title)),
        )
        .child(line(
            "From",
            value["from"].as_str().unwrap_or_default().to_owned(),
        ))
        .child(line("To", names(&to)))
        .when(!cc.is_empty(), |d| d.child(line("Cc", names(&cc))))
        .child(line(
            "Subject",
            value["subject"].as_str().unwrap_or_default().to_owned(),
        ))
        .child(
            div()
                .id(SharedString::from(format!("mono-mail-text-{id}")))
                .max_h(px(260.))
                .overflow_y_scroll()
                .rounded_md()
                .bg(content(0.04))
                .px_3()
                .py_2()
                .text_size(px(13.))
                .line_height(px(20.))
                .text_color(content(0.9))
                .whitespace_normal()
                .children(
                    text.lines()
                        .map(|l| div().min_h(px(20.)).child(l.to_owned()).into_any_element()),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .when(waiting, |d| {
                    d.child(
                        action(format!("mono-mail-send-{id}"), "Send", true)
                            .on_click(cx.listener(send("send"))),
                    )
                    .child(
                        action(format!("mono-mail-draft-{id}"), "Save as draft", false)
                            .on_click(cx.listener(send("draft"))),
                    )
                    .child(
                        action(format!("mono-mail-discard-{id}"), "Discard", false)
                            .on_click(cx.listener(send("discard"))),
                    )
                })
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(if state == "error" {
                            gpui::rgb(0xf87171).into()
                        } else {
                            content(0.55)
                        })
                        .child(match state.as_str() {
                            "sending" => "Sending…".to_owned(),
                            "saving" => "Saving…".to_owned(),
                            "sent" => "Sent from your account.".to_owned(),
                            "draft" => "Saved in Drafts on the server.".to_owned(),
                            "discarded" => "Discarded.".to_owned(),
                            "error" => value["error"]
                                .as_str()
                                .unwrap_or("It could not leave.")
                                .to_owned(),
                            _ => "Nothing leaves until you press Send.".to_owned(),
                        }),
                ),
        )
        .into_any_element()
}

/// The message the card holds, as the core sends it.
fn outgoing(card: &Value) -> Outgoing {
    Outgoing {
        to: people(&card["to"]),
        cc: people(&card["cc"]),
        subject: card["subject"].as_str().unwrap_or_default().to_owned(),
        text: card["text"].as_str().unwrap_or_default().to_owned(),
    }
}

impl Monos {
    /// Send, Save as draft or Discard on an agent's mail card: only the user's press.
    pub(super) fn mail_card_action(&mut self, mono: &str, action: &Value, cx: &mut Context<Self>) {
        let mut card = action["card"].clone();
        let Some(id) = card["id"].as_str().map(str::to_owned) else {
            return;
        };
        let what = action["do"].as_str().unwrap_or_default().to_owned();
        if !matches!(what.as_str(), "send" | "draft" | "discard")
            || !self.mail_sending.insert(id.clone())
        {
            return;
        }
        if what == "discard" {
            card["state"] = json!("discarded");
            self.patch_card(mono, &id, card, cx);
            self.mail_sending.remove(&id);
            return;
        }
        let mut busy = card.clone();
        busy["state"] = json!(if what == "send" { "sending" } else { "saving" });
        self.patch_card(mono, &id, busy, cx);
        let store = self.store.clone();
        let task = cx.background_executor().spawn({
            let card = card.clone();
            async move {
                let data_dir = locked(&store)?
                    .data_dir()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(model::desktop_data_dir);
                let service = model::Service::new(data_dir);
                let text = card["text"].as_str().unwrap_or_default();
                let subject = card["subject"].as_str();
                let all = card["all"] == true;
                match (card["mode"].as_str(), what.as_str()) {
                    (Some("reply"), "send") => service.send(
                        card["messageId"].as_str().unwrap_or_default(),
                        text,
                        subject,
                        all,
                    ),
                    (Some("reply"), _) => service.draft(
                        card["messageId"].as_str().unwrap_or_default(),
                        text,
                        subject,
                        all,
                    ),
                    (_, "send") => service.send_new(
                        card["accountId"].as_str().unwrap_or_default(),
                        &outgoing(&card),
                    ),
                    _ => service.draft_new(
                        card["accountId"].as_str().unwrap_or_default(),
                        &outgoing(&card),
                    ),
                }
                .map(|()| what)
            }
        });
        let mono = mono.to_owned();
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.mail_sending.remove(&id);
                match result {
                    Ok(what) => {
                        card["state"] = json!(if what == "send" { "sent" } else { "draft" });
                        card.as_object_mut().map(|c| c.remove("error"));
                    }
                    Err(error) => {
                        card["state"] = json!("error");
                        card["error"] = json!(error);
                    }
                }
                this.patch_card(&mono, &id, card, cx);
            });
        })
        .detach();
    }

    /// Replaces the card `id` in `mono`'s chat with `value`.
    pub(super) fn patch_card(
        &mut self,
        mono: &str,
        id: &str,
        value: Value,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = self.chats.get(mono).cloned() else {
            return;
        };
        let index = chat.read(cx).mono_rows(cx).iter().position(|row| {
            row.role == "mono-card"
                && serde_json::from_str::<Value>(&row.text)
                    .is_ok_and(|card| card["id"].as_str() == Some(id))
        });
        if let Some(index) = index {
            let _ = chat.update(cx, |chat, cx| chat.mono_patch_card(index, id, value, cx));
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_card_holds_the_message_the_core_sends() {
        let card = json!({
            "to": [{"name": "Luis", "address": "luis@x.test"}, {"address": "eva@x.test"}, {"name": "no address"}],
            "cc": [], "subject": "Gastos", "text": "Hola",
        });
        let out = outgoing(&card);
        assert_eq!(out.to.len(), 2);
        assert_eq!(names(&out.to), "Luis, eva@x.test");
        assert_eq!(out.subject, "Gastos");
        assert_eq!(state(&card), "");
    }
}
