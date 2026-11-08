//! The connected mail accounts (the desktop app's MailAccounts): who can read each one,
//! removing one, and the form that connects another (`connect.rs`). The password goes
//! straight to the system's store; MonoBOT only reads mail and sends the replies the user
//! sends.
use super::*;

/// The 36×20 switch of the app's settings.
fn switch(id: &'static str, label: &'static str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .role(Role::Switch)
        .aria_label(label)
        .aria_toggled(if on {
            gpui::Toggled::True
        } else {
            gpui::Toggled::False
        })
        .relative()
        .h(px(20.))
        .w(px(36.))
        .flex_shrink_0()
        .cursor_pointer()
        .bg(if on { accent_color() } else { content(0.2) })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 18. } else { 2. }))
                .size(px(16.))
                .bg(rgb(0xffffff)),
        )
}

impl Mailbox {
    /// Lets only `readers` read the account at `address` (every agent with All).
    fn set_readers(&mut self, address: String, readers: connect::Readers, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let ids = readers.ids();
        let task = cx.background_executor().spawn(async move {
            service
                .set_readers(&address, ids)
                .map(|()| service.access())
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(access) => this.access = access,
                    Err(error) => this.error = Some(error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn remove_account(&mut self, id: String, cx: &mut Context<Self>) {
        let service = self.service.clone();
        let task = cx
            .background_executor()
            .spawn(async move { service.remove_account(&id) });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                if let Err(error) = result {
                    this.error = Some(error);
                }
                this.overview = None;
                this.close_message(cx);
                this.load_accounts(cx);
            });
        })
        .detach();
    }

    /// The accounts, the pictures switch, and the form that connects another.
    pub(super) fn accounts_view(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let accounts = self.accounts.clone().unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(16.))
            .when(!accounts.is_empty(), |d| {
                d.child(div().flex().flex_col().gap(px(4.)).children(
                    accounts.iter().map(|account| {
                        let id = account.id.clone();
                        let photo = self.photos.get(&account.address.to_lowercase()).cloned();
                        let readers = connect::Readers::of(&self.access, &account.address);
                        let open = self.readers_open.as_deref() == Some(account.id.as_str());
                        let row = div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .text_size(px(13.))
                            .child(reply::face(
                                &account.name,
                                &account.address,
                                photo,
                                32.,
                                window,
                                cx,
                            ))
                            .child(
                                div()
                                    .min_w_0()
                                    .flex_1()
                                    .child(
                                        div()
                                            .truncate()
                                            .text_color(content(1.))
                                            .child(account.name.clone()),
                                    )
                                    .child(
                                        div()
                                            .truncate()
                                            .text_size(px(12.))
                                            .text_color(content(0.5))
                                            .child(account.address.clone()),
                                    ),
                            )
                            .child(
                                tool(
                                    SharedString::from(format!("mail-remove-{}", account.id)),
                                    &format!("Remove {}", account.address),
                                    "trash",
                                    content(0.5),
                                )
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_account(id.clone(), cx)
                                })),
                            );
                        self.account_readers(account, readers, open, row, cx)
                    }),
                ))
                .child(
                    div()
                        .flex()
                        .items_start()
                        .gap(px(12.))
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
                                        .text_size(px(13.))
                                        .text_color(content(1.))
                                        .child("Load pictures automatically"),
                                )
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .line_height(px(19.))
                                        .text_color(content(0.55))
                                        .child("MonoBOT fetches them for the message, with no cookies. A sender can still tell you opened it. Mail held back as suspicious never loads them on its own."),
                                ),
                        )
                        .child(
                            switch(
                                "mail-auto-images",
                                "Load pictures automatically",
                                self.auto_images,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.set_auto_images(!this.auto_images, cx)
                            })),
                        ),
                )
            })
            .child(self.form.clone())
            .into_any_element()
    }

    /// One account: its row, and who can read it (all agents, or the ones chosen), which
    /// opens to the agents' pills. The bots cannot change it; only the user, here.
    fn account_readers(
        &self,
        account: &AccountView,
        readers: connect::Readers,
        open: bool,
        row: Div,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let id = account.id.clone();
        let address = account.address.clone();
        let weak = cx.weak_entity();
        div()
            .id(SharedString::from(format!(
                "mail-account-row-{}",
                account.id
            )))
            .flex()
            .flex_col()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_1()
            .border_color(content(0.1))
            .child(row)
            .when(!self.agents.is_empty(), |d| {
                d.child(
                    div()
                        .id(SharedString::from(format!("mail-readers-{}", account.id)))
                        .debug_selector({
                            let id = format!("mail-readers-{}", account.id);
                            move || id.clone()
                        })
                        .role(Role::Button)
                        .aria_label(format!("Who can read {}", account.address))
                        .aria_expanded(open)
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(12.))
                        .text_color(content(0.55))
                        .cursor_pointer()
                        .hover(|s| s.text_color(content(0.85)))
                        .child(icon("square-lock", 12.))
                        .child(format!("Read by: {}", readers.summary(&self.agents)))
                        .child(icon(
                            if open {
                                "chevron-down"
                            } else {
                                "chevron-right"
                            },
                            12.,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.readers_open = if this.readers_open.as_deref() == Some(id.as_str())
                            {
                                None
                            } else {
                                Some(id.clone())
                            };
                            cx.notify();
                        })),
                )
                .when(open, |d| {
                    d.child(connect::readers_row(
                        &format!("mail-readers-pick-{}", account.id),
                        &self.agents,
                        &readers,
                        move |readers, _, cx| {
                            let address = address.clone();
                            let _ =
                                weak.update(cx, |this, cx| this.set_readers(address, readers, cx));
                        },
                    ))
                })
            })
    }
}
