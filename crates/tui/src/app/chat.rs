//! The conversation: sending a message, streaming the reply, and persisting both.

use super::*;

impl App {
    // ----- conversation -----------------------------------------------------

    pub(super) fn push_item(&mut self, item: Item) {
        self.items.push(item);
        self.cache.push(None);
    }

    pub(super) fn pop_item(&mut self) -> Option<Item> {
        self.cache.pop();
        self.items.pop()
    }

    pub(super) fn invalidate_last(&mut self) {
        if let Some(c) = self.cache.last_mut() {
            *c = None;
        }
    }

    pub fn messages(&self) -> impl DoubleEndedIterator<Item = &Message> {
        self.items.iter().filter_map(|i| match i {
            Item::Message(m) => Some(m),
            _ => None,
        })
    }

    pub(super) fn send_message(&mut self, text: String, tx: &Tx) {
        if self.turn_active() {
            self.notify("wait for the reply to finish, or press esc to cancel it");
            self.input.set_text(&text);
            return;
        }
        if self.current.is_none() {
            self.notify("no active model: /model to pick one");
            self.input.set_text(&text);
            return;
        }
        // snapshots of the files mentioned with @
        let mut attachments = Vec::new();
        for raw in mentions::extract(&text) {
            match Spec::parse(&raw).and_then(|s| self.read_spec(&s)) {
                Ok(a) => attachments.push(a),
                Err(e) => {
                    self.notify(format!("@{raw}: {e}"));
                    self.input.set_text(&text);
                    return;
                }
            }
        }
        let live = match self.read_live() {
            Ok(l) => l,
            Err(e) => {
                self.notify(e);
                self.input.set_text(&text);
                return;
            }
        };
        if let Err(e) = self.check_budget(&text, &attachments, &live) {
            self.notify(e);
            self.input.set_text(&text);
            return;
        }
        let mut msg = Message::user(text);
        msg.attachments = attachments;
        self.ensure_session(&msg.content);
        self.persist(&msg);
        self.push_item(Item::Message(msg));
        self.follow = true;
        // a new turn for the loop: its counters start over
        if let Some(h) = self.harness.as_mut() {
            h.begin_turn();
        }
        self.start_generation(tx);
    }

    pub(super) fn ensure_session(&mut self, first_text: &str) {
        if self.session.is_some() || !self.cfg.general.save_sessions {
            return;
        }
        let Some(store) = &self.store else { return };
        let model = self.current.as_ref().map(|c| c.qualified());
        match store.create(&title_from(first_text), model, self.system_prompt.clone()) {
            Ok(meta) => {
                self.touch_session(&meta.id);
                self.session = Some(meta);
                if self.tools_on {
                    self.update_session_meta();
                }
            }
            Err(e) => self.notify(format!("could not create the session: {e}")),
        }
    }

    pub(super) fn persist(&mut self, msg: &Message) {
        let (Some(store), Some(meta)) = (&self.store, &self.session) else {
            return;
        };
        if let Err(e) = store.append(meta, msg) {
            self.notify(format!("could not save: {e}"));
        }
    }

    pub(super) fn rewrite_session(&mut self) {
        let (Some(store), Some(meta)) = (&self.store, &self.session) else {
            return;
        };
        let msgs: Vec<Message> = self.messages().cloned().collect();
        if let Err(e) = store.rewrite(meta, &msgs) {
            self.notify(format!("could not save: {e}"));
        }
    }

    pub(super) fn update_session_meta(&mut self) {
        let (edit, create) = self.tools_scope();
        let (Some(store), Some(meta)) = (&self.store, &mut self.session) else {
            return;
        };
        meta.model = self.current.as_ref().map(|c| c.qualified());
        meta.system_prompt = self.system_prompt.clone();
        meta.attachments = self.live.iter().map(|s| s.to_string()).collect();
        meta.tools = self.tools_on;
        (meta.tools_edit, meta.tools_create) = (edit, create);
        if let Err(e) = store.update_meta(meta) {
            tracing::warn!(error = %e, "could not update the session");
        }
    }

    pub(super) fn start_generation(&mut self, tx: &Tx) {
        let Some(cur) = self.current.clone() else {
            return;
        };
        let Some(provider) = self.registry.get(&cur.provider) else {
            self.push_item(Item::Error(format!(
                "provider unavailable: {}",
                cur.provider
            )));
            self.abort_turn();
            return;
        };
        let live = match self.read_live() {
            Ok(l) => l,
            Err(e) => {
                self.push_item(Item::Error(e));
                self.abort_turn();
                return;
            }
        };
        let mut messages = Vec::new();
        if let Some(sp) = self.system_prompt_for(&live) {
            messages.push(Message::system(sp));
        }
        messages.extend(
            self.messages()
                .filter(|m| {
                    !m.content.is_empty()
                        || !m.attachments.is_empty()
                        || !m.tool_calls.is_empty()
                        || m.role == Role::Tool
                })
                .map(|m| {
                    let mut w = Message::new(m.role, m.content.clone());
                    w.attachments = m.attachments.clone();
                    w.tool_calls = m.tool_calls.clone();
                    w.tool_name = m.tool_name.clone();
                    w.tool_call_id = m.tool_call_id.clone();
                    w
                }),
        );
        let tools = match (&self.harness, self.tools_on) {
            (Some(h), true) => h.specs(),
            _ => Vec::new(),
        };
        let req = ChatRequest {
            model: cur.model.clone(),
            messages,
            params: self.params.clone(),
            tools,
        };
        self.last_run = None;
        let sent = self.estimated_request_tokens("", &[], &live);
        let cancel = CancellationToken::new();
        self.gen_id += 1;
        let id = self.gen_id;
        self.gen = Generation::Streaming {
            cancel: cancel.clone(),
            started: Instant::now(),
            first_at: None,
            deltas: 0,
            sent,
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let send = |ev: StreamEvent| {
                let _ = tx.send(Action::Stream(id, ev));
            };
            let mut stream = match provider.chat(req, cancel.clone()).await {
                Ok(s) => s,
                Err(ProviderError::Cancelled) => return send(StreamEvent::Cancelled),
                Err(e) => return send(StreamEvent::Error(e.to_string())),
            };
            while let Some(ev) = stream.next().await {
                match ev {
                    Ok(ChatEvent::Delta(d)) => send(StreamEvent::Delta(d)),
                    Ok(ChatEvent::Thinking(t)) => send(StreamEvent::Thinking(t)),
                    Ok(ChatEvent::ToolCall(c)) => send(StreamEvent::ToolCall(c)),
                    Ok(ChatEvent::Done(u)) => return send(StreamEvent::Done(u)),
                    Err(ProviderError::Cancelled) => return send(StreamEvent::Cancelled),
                    Err(e) => return send(StreamEvent::Error(e.to_string())),
                }
            }
            send(StreamEvent::Done(Usage::default()));
        });
    }

    /// Ensures the last item is the reply in progress.
    pub(super) fn streaming_message(&mut self) -> &mut Message {
        let is_open = matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::Assistant && m.usage.is_none() && !m.partial);
        if !is_open {
            let mut m = Message::assistant("");
            m.model = self.current.as_ref().map(|c| c.qualified());
            self.push_item(Item::Message(m));
        }
        self.invalidate_last();
        match self.items.last_mut() {
            Some(Item::Message(m)) => m,
            _ => unreachable!("the message was just ensured above"),
        }
    }

    pub(super) fn on_stream(&mut self, ev: StreamEvent, tx: &Tx) {
        match ev {
            StreamEvent::Delta(d) => {
                if let Generation::Streaming {
                    first_at, deltas, ..
                } = &mut self.gen
                {
                    first_at.get_or_insert_with(Instant::now);
                    *deltas += 1;
                }
                self.streaming_message().content.push_str(&d);
            }
            StreamEvent::Thinking(t) => {
                if let Generation::Streaming { first_at, .. } = &mut self.gen {
                    first_at.get_or_insert_with(Instant::now);
                }
                self.streaming_message()
                    .thinking
                    .get_or_insert_with(String::new)
                    .push_str(&t);
            }
            StreamEvent::ToolCall(c) => {
                if let Generation::Streaming { first_at, .. } = &mut self.gen {
                    first_at.get_or_insert_with(Instant::now);
                }
                self.streaming_message().tool_calls.push(c);
            }
            StreamEvent::Done(usage) => {
                self.finish_generation(Some(usage), false);
                self.after_reply(tx);
            }
            StreamEvent::Cancelled => {
                self.finish_generation(None, true);
                self.abort_turn();
            }
            StreamEvent::Error(e) => {
                self.finish_generation(None, false);
                self.push_item(Item::Error(e));
                self.abort_turn();
            }
        }
    }

    pub(super) fn finish_generation(&mut self, usage: Option<Usage>, cancelled: bool) {
        let summary = match std::mem::replace(&mut self.gen, Generation::Idle) {
            Generation::Streaming {
                started,
                first_at,
                deltas,
                sent,
                ..
            } => {
                let u = usage.as_ref();
                let prompt = u.and_then(|u| u.prompt_tokens);
                let received = u.and_then(|u| u.completion_tokens).unwrap_or(deltas);
                Some(RunSummary {
                    elapsed: started.elapsed(),
                    sent: prompt.unwrap_or(sent),
                    sent_estimated: prompt.is_none(),
                    received,
                    tok_per_s: first_at
                        .filter(|_| received > 0)
                        .map(|f| received as f32 / f.elapsed().as_secs_f32().max(0.1)),
                    cancelled,
                })
            }
            Generation::Idle => None,
        };
        let open = matches!(self.items.last(), Some(Item::Message(m)) if m.role == Role::Assistant && m.usage.is_none() && !m.partial);
        if !open {
            // cancelled before the first token: no message, but still a wrap-up
            if cancelled {
                match summary {
                    Some(s) => self.last_run = Some(s),
                    None => self.notify("generation cancelled"),
                }
            }
            return;
        }
        let empty = matches!(self.items.last(), Some(Item::Message(m)) if m.content.is_empty() && m.thinking.is_none() && m.tool_calls.is_empty());
        if empty {
            self.pop_item();
            if !cancelled && usage.is_some() {
                self.push_item(Item::Info("(empty reply)".into()));
            }
            self.last_run = summary;
            return;
        }
        let msg = {
            let Some(Item::Message(m)) = self.items.last_mut() else {
                return;
            };
            m.partial = cancelled;
            let mut u = usage.clone().unwrap_or_default();
            // if the provider gives no duration, the on-screen measure will do:
            // that way the session's accumulated time counts every request
            if u.total_duration_ms.is_none() {
                u.total_duration_ms = summary.as_ref().map(|s| s.elapsed.as_millis() as u64);
            }
            m.usage = Some(u);
            m.clone()
        };
        self.invalidate_last();
        self.last_run = summary;
        self.last_usage = usage;
        self.persist(&msg);
    }

    pub fn cancel_generation(&mut self) {
        if let Generation::Streaming { cancel, .. } = &self.gen {
            cancel.cancel();
            self.gen_id += 1; // ignore whatever arrives late
            self.finish_generation(None, true);
        }
        self.abort_turn();
    }
}
