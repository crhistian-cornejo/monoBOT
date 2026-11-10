//! The dialog a bot opens with `mail.connect`: the Email tab's connect form, filled in with
//! what the bot asked for, over the whole surface. The mailbox's owner types the app password
//! there (it goes to the system's store of secrets) and picks which agents read it. When it
//! connects, the bot hears so in its chat and carries on; it never sees the password.
use super::*;
use crate::inbox::mail::connect::{Agent, ConnectEvent, ConnectForm, Readers};
use monocode_core::inbox::mail::{self as model, ConnectRequest};

impl Monos {
    pub(super) fn open_mail_connect(
        &mut self,
        mono_id: &str,
        data_dir: PathBuf,
        request: ConnectRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A dialog the user is typing in stays; the bot can ask again later.
        if self
            .mail_connect
            .as_ref()
            .is_some_and(|(_, form, _, _)| form.read(cx).busy())
        {
            return;
        }
        let Some(mono) = self.roster.get(mono_id) else {
            return;
        };
        let bot = Agent {
            id: mono.id.clone(),
            label: mono.label(),
        };
        let agents = crate::inbox::mail::connect::agents_of(&self.roster);
        let service = model::Service::new(data_dir);
        let view =
            cx.new(|cx| ConnectForm::asked(service, &request, bot.clone(), agents, window, cx));
        let events =
            cx.subscribe_in(
                &view,
                window,
                |this, form, event: &ConnectEvent, _, cx| match event {
                    ConnectEvent::Close => this.close_mail_connect(cx),
                    ConnectEvent::Connected(address) => {
                        let readers = form.read(cx).readers().clone();
                        let Some((bot, ..)) = this.mail_connect.take() else {
                            return;
                        };
                        let text = connected_note(address, &readers, &bot);
                        this.enqueue(&bot, text, Vec::new(), true, cx);
                        cx.notify();
                    }
                },
            );
        self.mail_connect = Some((mono_id.to_owned(), view, events, window.viewport_size()));
        cx.notify();
    }

    fn close_mail_connect(&mut self, cx: &mut Context<Self>) {
        if self.mail_connect.take().is_some() {
            cx.notify();
        }
    }

    pub(super) fn mail_connect_modal(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (_, view, _, size) = self.mail_connect.as_ref()?;
        let weak = cx.weak_entity();
        let form = view.downgrade();
        Some(self.centered_dialog(
            "mail-connect",
            *size,
            (480., 640.),
            None,
            view.clone().into_any_element(),
            move |_, cx| {
                // A press outside never drops a connection on its way.
                if form.upgrade().is_some_and(|form| form.read(cx).busy()) {
                    return;
                }
                let _ = weak.update(cx, |this, cx| this.close_mail_connect(cx));
            },
        ))
    }
}

/// What the bot hears once the mailbox is connected: no secret, only who reads it.
fn connected_note(address: &str, readers: &Readers, bot: &str) -> String {
    let who = match readers {
        Readers::All => "every agent".to_owned(),
        Readers::Only(ids) if ids.iter().any(|id| id == bot) => {
            if ids.len() == 1 {
                "you alone".to_owned()
            } else {
                "you and other agents the user chose".to_owned()
            }
        }
        Readers::Only(_) => "other agents the user chose, not you".to_owned(),
    };
    format!(
        "The user connected {address} in monoBOT's own dialog; its app password is in the system's keychain. It is read by {who}. If you may read it, check it with mail.search and its account, then carry on with what you were doing."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bot_hears_who_reads_the_mailbox_and_never_a_secret() {
        let only = connected_note("ana@gmail.com", &Readers::Only(vec!["fin".into()]), "fin");
        assert!(only.contains("ana@gmail.com") && only.contains("you alone"));
        assert!(connected_note("a@b.c", &Readers::All, "fin").contains("every agent"));
        assert!(
            connected_note("a@b.c", &Readers::Only(vec!["x".into()]), "fin").contains("not you")
        );
    }
}
