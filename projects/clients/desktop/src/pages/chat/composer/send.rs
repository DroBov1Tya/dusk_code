//! Sending the message, queued while the agent works, and stopping the turn.

use gpui_kit::*;

use crate::{pages::chat::transcript::Transcript, workspace::Workspace};

impl Workspace {
    /// Sends the composer's text to the shown chat, or to a new chat of the selected
    /// project when none is shown.
    pub fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.is_none() && self.drafts.starting() {
            return;
        }
        let mut text = self.composer.read(cx).value().trim().to_owned();
        if self.run_command(&text, window, cx) {
            return;
        }
        let draft = super::Draft {
            text: self.composer.read(cx).value().to_string(),
            attachments: self.attachments.clone(),
            pastes: self.pastes.clone(),
        };
        for pasted in std::mem::take(&mut self.pastes) {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&dusk_protocol::thread::wrap(&pasted));
        }
        // Attached files go as their paths; the core keeps them among the chat's sources.
        for path in std::mem::take(&mut self.attachments) {
            if !text.is_empty() {
                text.push('\n');
            }
            // A path with spaces goes quoted, so the core reads it as one.
            let path = path.display().to_string();
            match path.contains(char::is_whitespace) && !path.contains('"') {
                true => text.push_str(&format!("\"{path}\"")),
                false => text.push_str(&path),
            }
        }
        if text.is_empty() {
            return;
        }
        let shown = self.shown.clone();
        self.drop_suggestion(shown.as_deref());
        // A pending question takes the text as the user's own answer.
        let asked = shown
            .as_ref()
            .and_then(|id| self.chats.get_mut(id))
            .filter(|chat| !chat.questions.is_empty());
        if let Some(chat) = asked {
            match chat.answer(&[], &text) {
                Some(answers) => text = answers,
                // More questions wait: this answer is kept and the next one shows.
                None => {
                    self.composer
                        .update(cx, |composer, cx| composer.set_value("", window, cx));
                    cx.notify();
                    return;
                }
            }
        } else if let Some(thread_id) = shown
            .clone()
            .filter(|id| self.chats.get(id).is_some_and(|chat| chat.working))
        {
            // The agent is busy: the text waits, editable, until the turn ends.
            self.composer
                .update(cx, |composer, cx| composer.set_value("", window, cx));
            self.queue(thread_id, text, window, cx);
            return;
        }
        // Without a project, a new chat works in the workspace root.
        let dir = Some(
            self.selected_project()
                .map_or_else(|| self.home.clone(), |project| project.dir.clone()),
        );
        self.composer
            .update(cx, |composer, cx| composer.set_value("", window, cx));
        let temporary = std::mem::take(&mut self.temporary_next);
        let origin = shown.clone();
        let generation = self.drafts.generation();
        let navigation = self.history.navigation();
        if origin.is_none() {
            self.drafts.begin();
        }
        // A chat already on the page catches its turn's events as they come.
        if let Some(thread_id) = shown {
            self.deliver(thread_id, text, origin, generation, draft, cx);
            return;
        }
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                match dir {
                    Some(dir) if temporary => threads.start_temporary(dir).await,
                    Some(dir) => threads.start(dir).await,
                    None => anyhow::bail!("no project"),
                }
            },
            move |ws, result, cx| {
                ws.drafts.finish(generation);
                match result {
                    Ok(thread_id) => {
                        // The chat is on the page before the message is queued, so the
                        // turn it starts is seen from its first event.
                        let current = ws.drafts.started(&thread_id, generation);
                        ws.chats.insert(thread_id.clone(), Transcript::default());
                        if current
                            && ws.history.navigation() == navigation
                            && ws.shown.is_none()
                            && ws.center == crate::workspace::Center::Chat
                        {
                            ws.show(thread_id.clone(), cx);
                        }
                        ws.reload_threads(cx);
                        ws.deliver(thread_id, text, None, generation, draft, cx);
                    }
                    Err(error) => {
                        ws.edit_draft_at(None, generation, super::Change::Restore(draft), cx);
                        ws.fail(error, cx);
                    }
                }
            },
        );
    }

    /// Gives the text to a thread that is already on the page; a failed send puts the
    /// draft back where it was written.
    fn deliver(
        &mut self,
        thread_id: String,
        text: String,
        origin: Option<String>,
        generation: u64,
        draft: super::Draft,
        cx: &mut Context<Self>,
    ) {
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, false).await },
            move |ws, result, cx| match result {
                Ok(()) => ws.load_sources(cx),
                Err(error) => {
                    ws.edit_draft_at(origin, generation, super::Change::Restore(draft), cx);
                    ws.fail(error, cx);
                }
            },
        );
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let (turn, agents) = self.shown_chat().map_or((false, Vec::new()), |chat| {
            let agents = chat
                .agents
                .iter()
                .filter(|(_, agent)| agent.working())
                .map(|(id, _)| id.clone())
                .collect();
            (chat.working, agents)
        });
        let (chat_id, stopped) = (thread_id.clone(), agents.clone());
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                if turn {
                    threads.cancel(&thread_id).await?;
                }
                for agent in agents {
                    threads.stop_agent(&agent).await?;
                }
                anyhow::Ok(())
            },
            move |ws, result, cx| match result {
                Ok(()) => {
                    // A stopped agent may end without another event; the view must not wait.
                    if let Some(chat) = ws.chats.get_mut(&chat_id) {
                        for id in &stopped {
                            if let Some(agent) = chat.agents.get_mut(id) {
                                agent.stop(dusk_protocol::system::now_ms());
                            }
                        }
                    }
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
