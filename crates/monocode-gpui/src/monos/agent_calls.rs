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
    /// A group member asked with group.ask: the answer names it too.
    member: bool,
}

impl AgentRoute {
    /// What the waiting call returns; a group member's names it as such.
    fn waited(&self, id: &str, reply: Option<&str>) -> Value {
        let mut value = agents::waited(id, &self.name, reply);
        if self.member {
            value["member"] = self.name.clone().into();
        }
        value
    }
}

#[derive(Default)]
pub(super) struct AgentCalls {
    pub(super) messaging: Messaging,
    delegations: Option<Vec<Delegation>>,
    /// By the outbox message that carried each request.
    routes: HashMap<String, AgentRoute>,
    writes: Option<gpui::Task<()>>,
    completed: std::collections::VecDeque<String>,
}

impl AgentCalls {
    /// Whether a live route is still waiting on this delegation's answer:
    /// the panel's "awaiting reply" mark. Routes live in memory, so a
    /// restart drops them and a stale entry shows nothing rather than
    /// claim a wait that is no longer running.
    pub(super) fn pending_delegation(&self, id: &str) -> bool {
        self.routes
            .values()
            .any(|route| route.delegation.as_deref() == Some(id))
    }

    pub(super) fn retry_route(&mut self, previous: &str, next: &str) {
        if let Some(route) = self.routes.remove(previous) {
            self.routes.insert(next.to_owned(), route);
        }
    }

    #[cfg(test)]
    pub(super) fn forget_routes(&mut self) {
        self.routes.clear();
        self.delegations = None;
        self.completed.clear();
    }
}

impl Monos {
    pub(super) fn delegations(&mut self) -> &mut Vec<Delegation> {
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
        let previous = self.agent_calls.writes.take();
        self.agent_calls.writes = Some(cx.spawn(async move |this, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { locked(&store).and_then(|db| delegations::save(&db, &list)) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.error = Some(error);
                    cx.notify();
                });
            }
        }));
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
            "tools.read" | "tools.update" => self.tools_call(mono, action, input, cx),
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

    /// A bot reads and changes its own services and skills (handleTools).
    fn tools_call(
        &mut self,
        mono: &Mono,
        action: &str,
        input: &Value,
        cx: &mut Context<Self>,
    ) -> Result<Value, String> {
        if !mono.is_bot() {
            return Err("Only a bot's own conversation can manage its tools".into());
        }
        let role = mono.bot_role.clone().unwrap_or_else(|| "assistant".into());
        let rows = monocode_core::connectors::tool_rows();
        if action == "tools.update" {
            let ids: Vec<String> = rows.iter().map(|r| r.0.clone()).collect();
            let granted: Vec<(String, String)> = rows
                .iter()
                .filter(|r| r.4)
                .map(|r| (r.0.clone(), r.1.clone()))
                .collect();
            let current = monocode_core::mono::bot_tools::tools(&role, mono.bot_tools.as_ref());
            let next = agents::required_tools(input, &current, &ids)?;
            agents::refuse_user_granted(
                next.connectors.as_deref(),
                current.connectors.as_deref(),
                &granted,
            )?;
            if let Some(bot) = self.roster.get_mut(&mono.id) {
                bot.bot_tools = Some(next);
            }
            self.save(cx);
            self.changed(cx);
        }
        let own = self.roster.get(&mono.id).and_then(|m| m.bot_tools.clone());
        let tools = monocode_core::mono::bot_tools::tools(&role, own.as_ref());
        let home = monocode_core::accounts::home().unwrap_or_default();
        let installed = monocode_core::mono::bot_tools::installed_skills(&home);
        Ok(agents::tools_view(
            &tools,
            &installed,
            &rows,
            action == "tools.update",
        ))
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
        let delegation =
            delegations::record(self.delegations(), &sender.id, &target.id, &message, now);
        let text = agents::agent_message_with_delegation(
            &sender.label(),
            &agents::member_role(sender),
            &message,
            Some(&delegation),
        );
        let Some(message_id) = self.enqueue(&target.id, text, Vec::new(), false, cx) else {
            self.delegations().retain(|entry| entry.id != delegation);
            self.finish_call(
                call,
                Err("That agent's conversation is not available".into()),
                cx,
            );
            return None;
        };
        self.ensure_chat(&target.id, cx);
        self.agent_calls.messaging.mark_answering(&target.id, now);
        self.save_delegations(cx);
        let mut route = AgentRoute {
            from_mono: sender.id.clone(),
            from_session: call.session,
            name: target.label(),
            wait: wait.then(|| call.request_id.clone()),
            delegation: Some(delegation),
            call: None,
            reply: None,
            member: false,
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
            self.finish_call(call, Ok(route.waited(&id, Some(&reply))), cx);
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
                let pending = route.waited(&id, None);
                this.finish_call(call, Ok(pending), cx);
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

    /// The answer an agent wrote after the message `sent` landed, as
    /// Tauri's replyAfter reads it: every assistant row until the next
    /// user message. The clipped notification body would cut a longer
    /// answer at its first paragraph, so it is not used here.
    fn agent_reply(&self, chat: &str, sent: &str, cx: &App) -> Option<String> {
        let view = self.chats.get(chat)?;
        let rows = view.read(cx).mono_rows(cx);
        let message = self.inflight.get(chat).filter(|m| m.id == sent)?;
        let start = {
            rows.iter().enumerate().rev().find_map(|(index, row)| {
                if !row.is_prompt() || !row.first {
                    return None;
                }
                let text: String = rows[index..]
                    .iter()
                    .take_while(|part| {
                        part.role == row.role && (std::ptr::eq(*part, row) || !part.first)
                    })
                    .map(|part| part.text.as_str())
                    .collect();
                (text.trim() == message.text.trim()).then_some(index)
            })
        }?;
        agents::reply_after(rows, start, false).map(|(_, reply)| reply)
    }

    /// The other agent's turn ended after the message `sent` landed: its
    /// answer goes to the call waiting for it, or to the asker's chat.
    pub(super) fn agent_answered(
        &mut self,
        chat: &str,
        sent: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.agent_calls.completed.iter().any(|id| id == sent) {
            return true;
        }
        if self.agent_calls.routes.contains_key(sent) {
            let Some(reply) = self.agent_reply(chat, sent, cx) else {
                return false;
            };
            self.agent_calls.completed.push_back(sent.to_owned());
            if self.agent_calls.completed.len() > delegations::MAX {
                self.agent_calls.completed.pop_front();
            }
            self.route_answered(sent, reply, cx);
            return true;
        }
        // Routes live in memory, so a restart loses them while the queued
        // agent_message survives: the answer still goes to the agent the
        // message names, else the asker would wait on it forever.
        let Some(text) = self
            .inflight
            .get(chat)
            .filter(|m| m.id == sent)
            .map(|m| m.text.clone())
        else {
            return true;
        };
        let Some(from) = agents::message_sender(&text) else {
            return true;
        };
        let delegation_id = agents::message_delegation(&text).map(str::to_owned);
        let stored_sender = delegation_id.as_deref().and_then(|id| {
            self.delegations()
                .iter()
                .find(|entry| entry.id == id && entry.to == chat)
                .map(|entry| entry.from.clone())
        });
        let asker = stored_sender
            .as_deref()
            .and_then(|id| self.roster.get(id).cloned())
            .or_else(|| {
                agents::find_agent(chat, &self.roster.monos, from)
                    .cloned()
                    .ok()
            });
        let Some(asker) = asker else {
            return false;
        };
        let Some(reply) = self.agent_reply(chat, sent, cx) else {
            return false;
        };
        self.agent_calls.completed.push_back(sent.to_owned());
        if self.agent_calls.completed.len() > delegations::MAX {
            self.agent_calls.completed.pop_front();
        }
        let reply = if reply.trim().is_empty() {
            "(no written answer)".to_owned()
        } else {
            reply
        };
        let pending = self
            .delegations()
            .iter()
            .rev()
            .find(|d| {
                d.from == asker.id
                    && d.to == chat
                    && d.reply.is_none()
                    && delegation_id.as_deref().is_none_or(|id| id == d.id)
            })
            .map(|d| d.id.clone());
        if let Some(id) = pending
            && delegations::answer(self.delegations(), &id, &reply)
        {
            self.save_delegations(cx);
        }
        let name = self
            .roster
            .get(chat)
            .map_or_else(|| chat.to_owned(), Mono::label);
        let message = format!(
            "<agent_reply from=\"{}\">\n{}\n</agent_reply>\n{} answered the message you sent with agents.message.",
            name,
            reply.trim(),
            name
        );
        self.enqueue(&asker.id, message, Vec::new(), true, cx);
        self.ensure_chat(&asker.id, cx);
        true
    }

    /// A group's chat waits for one member's answer to group.ask, keyed by
    /// the request it went out as (waitedAnswer); agents.wait keeps waiting.
    pub(super) fn wait_for_member(
        &mut self,
        call: Call,
        from_mono: &str,
        request: String,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let route = AgentRoute {
            from_mono: from_mono.to_owned(),
            from_session: call.session,
            name,
            wait: Some(call.request_id.clone()),
            delegation: None,
            call: Some(call),
            reply: None,
            member: true,
        };
        self.agent_calls.routes.insert(request.clone(), route);
        self.expire_wait(request, cx);
    }

    /// An answer came back for the message or request `sent`.
    pub(super) fn route_answered(&mut self, sent: &str, reply: String, cx: &mut Context<Self>) {
        if !self.agent_calls.routes.contains_key(sent) {
            return;
        }
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
        let reply = if reply.trim().is_empty() {
            "(no written answer)".to_owned()
        } else {
            reply
        };
        self.answer_delegation(&route, &reply, cx);
        if let Some(call) = route.call.take() {
            let id = route.wait.clone().unwrap_or_default();
            self.finish_call(call, Ok(route.waited(&id, Some(&reply))), cx);
            return;
        }
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
