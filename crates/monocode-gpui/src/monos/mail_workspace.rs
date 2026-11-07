use super::*;
use monocode_core::inbox::workspace;

impl Monos {
    pub(super) fn poll_cloud_mail(&mut self, cx: &mut Context<Self>) {
        let now = monocode_core::time::now_ms();
        if cx.is_test() || self.mail_polling || now.saturating_sub(self.mail_last_poll) < 30_000 {
            return;
        }
        self.mail_polling = true;
        self.mail_last_poll = now;
        let store = self.store.clone();
        let work = cx.background_executor().spawn(async move {
            let dir = locked(&store)?
                .data_dir()
                .map(PathBuf::from)
                .unwrap_or_else(monocode_core::inbox::mail::desktop_data_dir);
            let service = workspace::Service::new(dir);
            let mut deliveries = service.sync_inbound()?;
            if !deliveries.is_empty() {
                let db = locked(&store)?;
                let roster = Roster::load(&db)?;
                let mut completed = Vec::new();
                for delivery in &deliveries {
                    if let Some(session) =
                        roster.get(&delivery.bot_id).and_then(|bot| bot.session_id)
                    {
                        if db
                            .setting(&format!("mono-delivered:{session}:{}", delivery.id))?
                            .is_some()
                        {
                            completed.push(delivery.id.clone());
                        }
                    }
                }
                drop(db);
                service.ack_deliveries(&completed)?;
                deliveries.retain(|delivery| !completed.contains(&delivery.id));
            }
            Ok::<_, String>(deliveries)
        });
        cx.spawn(async move |this, cx| {
            let deliveries = work.await;
            let _ = this.update(cx, |this, cx| {
                this.mail_polling = false;
                let deliveries = match deliveries {
                    Ok(value) => value,
                    Err(error) => {
                        this.error = Some(error);
                        cx.notify();
                        return;
                    }
                };
                for delivery in deliveries {
                    let Some(bot) = this.roster.get_mut(&delivery.bot_id).filter(|m| m.is_bot())
                    else {
                        continue;
                    };
                    if bot.unavailable_session.is_some() {
                        continue;
                    }
                    if !bot.outbox.iter().any(|q| q.id == delivery.id) {
                        bot.outbox.push(Message {
                            id: delivery.id,
                            text: delivery.prompt,
                            sent_at: Some(now),
                            internal: false,
                            error: None,
                            attachments: vec![],
                        });
                    }
                    // Save uses the ordinary ordered outbox persistence. It
                    // acknowledges cloud delivery only after roster.save
                    // succeeds, and only then allows dispatch.
                    this.save(cx);
                    this.ensure_chat(&delivery.bot_id, cx);
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cloud_mail_waits_for_a_separate_turn_and_failed_delivery_stays_blocked() {
        let mut message = Message {
            id: "cloudmail-source-1".into(),
            text: "Untrusted email JSON".into(),
            sent_at: None,
            internal: false,
            error: None,
            attachments: vec![],
        };
        assert!(can_dispatch(&message, false));
        assert!(!can_dispatch(&message, true));
        message.error = Some("Retry required".into());
        assert!(!can_dispatch(&message, false));
    }
}
