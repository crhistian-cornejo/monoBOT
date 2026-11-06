//! The agents' own app actions (agentApp.ts handleAgents, handleBots and
//! name.update): one agent writing to another and waiting for its answer, the
//! Principal listing and making bots, an agent renaming itself. The decisions
//! are core's (monocode_core::mono::agents); this keeps the chats and calls.
use super::*;
use monocode_core::mono::{
    agents::{self, Messaging},
    bridge::Call,
    delegations::{self, Delegation},
    files,
};
use monocode_core::time::now_ms;
use serde_json::{Value, json};

/// A message one agent sent another, until its answer comes back.
pub(super) struct AgentRoute {
    from_mono: String,
    from_session: i64,
    /// The recipient's name, as the answer is signed.
    name: String,
    /// The waitId, when the asker waits for the answer.
    wait: Option<String>,
    delegation: Option<String>,
    /// A call waiting right now for this answer.
    call: Option<Call>,
    /// The answer, once it came while no call was waiting.
    reply: Option<String>,
}

#[derive(Default)]
pub(super) struct AgentCalls {
    pub(super) messaging: Messaging,
    delegations: Option<Vec<Delegation>>,
    /// By the outbox message that carried each request.
    routes: HashMap<String, AgentRoute>,
}

impl Monos {
    fn delegations(&mut self) -> &mut Vec<Delegation> {
        let store = self.store.clone();
        self.agent_calls.delegations.get_or_insert_with(|| {
            locked(&store)
                .map(|db| delegations::load(&db))
                .unwrap_or_default()
        })
    }
    fn save_delegations(&mut self, cx: &mut Context<Self>) {
        let list = self.delegations().clone();
        let store = self.store.clone();
        cx.background_executor()
            .spawn(async move {
                let _ = locked(&store).and_then(|db| delegations::save(&db, &list));
            })
            .detach();
    }

    /// Whether this action is one of the agents' own; answers it if so.
    pub(super) fn agent_call(
        &mut self,
        call: Call,
        mono: &Mono,
        action: &str,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Option<Call> {
        let result = match action {
            "bots.list" | "bots.create" => self.bots_call(mono, action, input, cx),
            "name.update" => agents::new_name(input).map(|name| {
                if let Some(entry) = self.roster.get_mut(&mono.id) {
                    entry.name = name.clone();
                    entry.normalize();
                }
                self.save(cx);
                self.changed(cx);
                json!({
                    "name": name,
                    "note": "The rail, the tab and the chat show the new name now. Say it in one line.",
                })
            }),
            "agents.list" => {
                let recent = self.delegations().clone();
                Ok(agents::agents_list(&mono.id, &self.roster.monos, &recent, &[]))
            }
            "agents.message" => return self.agent_message(call, mono, input, cx),
            "agents.wait" => return self.agent_wait(call, input, cx),
            _ => return Some(call),
        };
        self.finish_agent_call(call, result, cx);
        None
    }

    fn finish_agent_call(
        &mut self,
        call: Call,
        result: Result<Value, String>,
        cx: &mut Context<Self>,
    ) {
        self.finish_call(call, result, cx);
    }

    /// The Principal creates and lists the user's personal bots (handleBots).
    fn bots_call(
        &mut self,
        mono: &Mono,
        action: &str,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !mono.is_principal() {
            return Err("Only the Principal bot can manage bots".into());
        }
        if action == "bots.list" {
            return Ok(json!({
                "bots": self.roster.monos.iter().filter(|m| m.is_bot()).map(agents::bot_view).collect::<Vec<_>>()
            }));
        }
        let made = agents::new_bot(input, &[], &[])?;
        let root = self.root.clone();
        let role = monocode_core::mono::bots::role(&made.role).ok_or("Unknown role")?;
        let id = self
            .roster
            .create_bot(&root, &made.role, false)
            .map(|m| m.id.clone())
            .ok_or("Could not create the bot")?;
        if let Some(bot) = self.roster.get_mut(&id) {
            bot.name = made.name.clone();
            bot.specialty = made.specialty.clone();
            bot.bot_tools = made.tools.clone();
            bot.normalize();
        }
        if made.instructions != role.instructions {
            files::save(&root, &id, None, "SOUL.md", &made.instructions, None)?;
        }
        self.save(cx);
        self.changed(cx);
        let bot = self.roster.get(&id).ok_or("Could not create the bot")?;
        let mut view = agents::bot_view(bot);
        view["specialty"] = made
            .specialty
            .unwrap_or_else(|| role.specialty.to_owned())
            .into();
        Ok(json!({ "bot": view }))
    }

    /// agents.message: signs the message, queues it in the other agent's
    /// chat and records it; the answer comes back to the call when it waits,
    /// else to the asker's chat when the other agent's turn ends.
    fn agent_message(
        &mut self,
        call: Call,
        sender: &Mono,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Option<Call> {
        let now = now_ms();
        let checked = (|| {
            let target = agents::find_agent(
                &sender.id,
                &self.roster.monos,
                input["to"].as_str().unwrap_or_default(),
            )?
            .clone();
            if self.agent_calls.messaging.is_answering(&sender.id, now) {
                return Err(
                    "You are answering another agent: reply to it instead of messaging others"
                        .to_owned(),
                );
            }
            if !call
                .request_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                || call.request_id.is_empty()
                || call.request_id.len() > 128
            {
                return Err("Invalid request ID".into());
            }
            let message = agents::required_string(input, "message", 30_000)?;
            Ok((target, message))
        })();
        let (target, message) = match checked {
            Ok(checked) => checked,
            Err(error) => {
                self.finish_call(call, Err(error), cx);
                return None;
            }
        };
        let wait = input["wait"] == true;
        let text = agents::agent_message(&sender.label(), &agents::member_role(sender), &message);
        let Some(message_id) = self.enqueue(&target.id, text, Vec::new(), false, cx) else {
            self.finish_call(
                call,
                Err("That agent's conversation is not available".into()),
                cx,
            );
            return None;
        };
        self.ensure_chat(&target.id, cx);
        self.agent_calls.messaging.mark_answering(&target.id, now);
        let delegation =
            delegations::record(self.delegations(), &sender.id, &target.id, &message, now);
        self.save_delegations(cx);
        let mut route = AgentRoute {
            from_mono: sender.id.clone(),
            from_session: call.session,
            name: target.label(),
            wait: wait.then(|| call.request_id.clone()),
            delegation: Some(delegation),
            call: None,
            reply: None,
        };
        if !wait {
            self.agent_calls.routes.insert(message_id, route);
            self.finish_call(
                call,
                Ok(json!({"to": target.label(), "submitted": true, "replyComesBack": true})),
                cx,
            );
            return None;
        }
        let wait_id = call.request_id.clone();
        route.call = Some(call);
        let _ = wait_id;
        self.agent_calls.routes.insert(message_id.clone(), route);
        self.expire_wait(message_id, cx);
        None
    }

    /// agents.wait: keeps waiting for an answer a message asked for.
    fn agent_wait(&mut self, call: Call, input: &Value, cx: &mut Context<Self>) -> Option<Call> {
        let id = match agents::required_string(input, "waitId", 128) {
            Ok(id) => id,
            Err(error) => {
                self.finish_call(call, Err(error), cx);
                return None;
            }
        };
        let found = self
            .agent_calls
            .routes
            .iter()
            .find(|(_, r)| r.wait.as_deref() == Some(id.as_str()) && r.from_session == call.session)
            .map(|(key, _)| key.clone());
        let Some(key) = found else {
            self.finish_call(call, Err("No answer is awaited by that waitId".into()), cx);
            return None;
        };
        if let Some(reply) = self
            .agent_calls
            .routes
            .get_mut(&key)
            .and_then(|r| r.reply.take())
        {
            let route = self.agent_calls.routes.remove(&key).unwrap();
            self.answer_delegation(&route, &reply, cx);
            self.finish_call(call, Ok(agents::waited(&id, &route.name, Some(&reply))), cx);
            return None;
        }
        if let Some(route) = self.agent_calls.routes.get_mut(&key) {
            route.call = Some(call);
        }
        self.expire_wait(key, cx);
        None
    }

    /// After WAIT_STEP_MS a waiting call returns how to keep waiting.
    fn expire_wait(&mut self, key: String, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(agents::WAIT_STEP_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                let Some(route) = this.agent_calls.routes.get_mut(&key) else {
                    return;
                };
                let Some(call) = route.call.take() else {
                    return;
                };
                let id = route.wait.clone().unwrap_or_default();
                let name = route.name.clone();
                this.finish_call(call, Ok(agents::waited(&id, &name, None)), cx);
            });
        })
        .detach();
    }

    fn answer_delegation(&mut self, route: &AgentRoute, reply: &str, cx: &mut Context<Self>) {
        if let Some(id) = &route.delegation
            && delegations::answer(self.delegations(), id, reply)
        {
            self.save_delegations(cx);
        }
    }

    /// The other agent's turn ended after the message `sent` landed: its
    /// answer goes to the call waiting for it, or to the asker's chat.
    pub(super) fn agent_answered(&mut self, chat: &str, sent: &str, cx: &mut Context<Self>) {
        if !self.agent_calls.routes.contains_key(sent) {
            return;
        }
        let reply = self
            .chats
            .get(chat)
            .map(|view| view.read(cx).notification_reply_body(cx))
            .unwrap_or_default();
        let waiting = self
            .agent_calls
            .routes
            .get(sent)
            .is_some_and(|route| route.wait.is_some() && route.call.is_none());
        if waiting {
            // The asker collects it with agents.wait.
            if let Some(route) = self.agent_calls.routes.get_mut(sent) {
                route.reply = Some(reply);
            }
            return;
        }
        let Some(mut route) = self.agent_calls.routes.remove(sent) else {
            return;
        };
        self.answer_delegation(&route, &reply, cx);
        if let Some(call) = route.call.take() {
            let id = route.wait.clone().unwrap_or_default();
            self.finish_call(call, Ok(agents::waited(&id, &route.name, Some(&reply))), cx);
            return;
        }
        let reply = if reply.trim().is_empty() {
            "(no written answer)".to_owned()
        } else {
            reply
        };
        let message = format!(
            "<agent_reply from=\"{}\">\n{}\n</agent_reply>\n{} answered the message you sent with agents.message.",
            route.name,
            reply.trim(),
            route.name
        );
        let from = route.from_mono.clone();
        self.enqueue(&from, message, Vec::new(), true, cx);
        self.ensure_chat(&from, cx);
    }
}
