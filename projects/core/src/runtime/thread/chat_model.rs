//! Which model a thread answers on. A chat has its own, kept with it: the last one chosen
//! when it started, then whatever is chosen in it. Its agents and its team start from the
//! chat's, so choosing a model in another chat never reaches them.

use std::sync::Arc;

use anyhow::Result;
use tracing::warn;

use crate::config::Effort;
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Input, Mailbox};
use crate::runtime::model_choice::{Choice, ModelChoice};

use super::Launcher;

/// What a chat answers on: its model, and the effort it thinks at.
#[derive(Clone)]
pub struct ChatModel {
    pub choice: Option<Choice>,
    pub effort: Option<Effort>,
}

impl ChatModel {
    /// What the chat of a running thread answers on now.
    pub fn of(mailbox: &Mailbox, launcher: &Launcher) -> Self {
        let model = &mailbox.context.model;
        Self {
            choice: model.current(),
            effort: model.effort_in(&launcher.config.current()),
        }
    }

    pub fn name(&self) -> Option<String> {
        self.choice.as_ref().map(Choice::name)
    }
}

impl Launcher {
    /// The selection a chat opens with: its own model and effort, or for a chat that has
    /// none yet the last chosen, which it keeps from then on.
    pub(crate) async fn chat_selection(&self, thread_id: &str) -> Result<Arc<ModelChoice>> {
        let config = self.config.current();
        let stored = self.state.chat_model(thread_id).await?;
        let choice = match &stored {
            Some(name) => self.named(name).or_else(|| {
                warn!("thread {thread_id}: {name} cannot be asked now; the last chosen answers");
                self.model.current()
            }),
            None => self.model.current(),
        };
        if stored.is_none()
            && let Some(choice) = &choice
        {
            self.state.set_chat_model(thread_id, &choice.name()).await?;
        }
        let effort = match self.state.chat_effort(thread_id).await? {
            Some(effort) => effort,
            None => {
                let effort = config.effort();
                self.state.set_chat_effort(thread_id, effort).await?;
                effort
            }
        };
        let own = self.model.own(choice);
        own.set_effort(Some(effort));
        self.learn_sizes(thread_id, &own);
        Ok(own)
    }

    /// What the chat `thread_id` belongs to answers on: its running selection, else what
    /// is stored with it, else the last chosen. The caller must not hold the mailboxes.
    pub(crate) async fn chat_model(&self, thread_id: &str) -> Result<ChatModel> {
        let chat = self.state.root_thread(thread_id).await?;
        match self.mailboxes.get(&chat).await {
            Some(mailbox) => Ok(ChatModel::of(&mailbox, self)),
            None => self.stored_chat_model(&chat).await,
        }
    }

    /// What a chat that is not running answers on when it is opened.
    pub(crate) async fn stored_chat_model(&self, chat: &str) -> Result<ChatModel> {
        let choice = match self.state.chat_model(chat).await? {
            Some(name) => self.named(&name),
            None => None,
        };
        let effort = self.state.chat_effort(chat).await?;
        Ok(ChatModel {
            choice: choice.or_else(|| self.model.current()),
            effort: effort.unwrap_or_else(|| self.config.current().effort()),
        })
    }

    /// `provider/model` as some thread knows it, or built from the config without asking
    /// the provider; `None` when its provider is gone or switched off.
    pub(crate) fn named(&self, name: &str) -> Option<Choice> {
        if let Some(known) = self.model.known(name) {
            return Some(known);
        }
        match Choice::named(&self.config.current(), name) {
            Ok(choice) => choice,
            Err(error) => {
                warn!("{name}: {error:#}");
                None
            }
        }
    }

    /// A model whose sizes no thread knows yet is asked for them in the background, and
    /// the chat told its window once they come.
    fn learn_sizes(&self, thread_id: &str, own: &Arc<ModelChoice>) {
        let Some(choice) = own.current() else {
            return;
        };
        if choice.model.context_window.is_some() {
            return;
        }
        let (own, launcher, thread_id) = (Arc::clone(own), self.clone(), thread_id.to_owned());
        tokio::spawn(async move {
            let model = match choice.refresh().await {
                Ok(model) => model,
                Err(error) => return warn!("thread {thread_id}: {error:#}"),
            };
            if let Some(learned) = own.refresh_model(&choice, model)
                && let Some(mailbox) = launcher.mailboxes.get(&thread_id).await
            {
                mailbox.emit(ThreadEvent::ModelSelected {
                    name: learned.name(),
                    input_limit: learned.input_limit(&launcher.config.current()),
                    effort: own.effort_in(&launcher.config.current()),
                });
            }
        });
    }

    /// Tells every running thread the model it answers on, and wakes the ones that can
    /// answer: a thread that had none may have one now.
    pub(crate) async fn announce_models(&self) {
        let config = self.config.current();
        for mailbox in self.mailboxes.lock().await.values() {
            let model = &mailbox.context.model;
            let Some(choice) = model.current() else {
                continue;
            };
            mailbox.emit(ThreadEvent::ModelSelected {
                name: choice.name(),
                input_limit: choice.input_limit(&config),
                effort: model.effort_in(&config),
            });
            mailbox.send(Input::Sent);
        }
    }
}
