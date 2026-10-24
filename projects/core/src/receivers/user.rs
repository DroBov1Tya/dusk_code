use anyhow::{Result, bail, ensure};

use crate::{
    Execution, Task,
    state::{MessageAttachment, State, Turn},
    transport::Transport,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserMessage {
    pub request_uid: String,
    pub thread_id: String,
    pub content: String,
    pub cwd: String,
    pub attachments: Vec<MessageAttachment>,
}

/// Reserved route for future slash-command scenarios. It is not a message turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScenarioRequest {
    pub request_uid: String,
    pub thread_id: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UserRequest {
    Message(UserMessage),
    Scenario(ScenarioRequest),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserReceipt {
    /// Transport acceptance metadata; the turn owns the subsequent lifecycle.
    pub execution: Execution,
    pub turn: Option<Turn>,
}

/// The user-facing entry point exposes message submission, without state setters.
pub struct UserReceiver<'a> {
    state: &'a State,
}

impl<'a> UserReceiver<'a> {
    pub fn new(state: &'a State) -> Self {
        Self { state }
    }

    pub async fn receive(&self, request: UserRequest) -> Result<UserReceipt> {
        match request {
            UserRequest::Message(message) => self.submit_message(message).await,
            UserRequest::Scenario(request) => self.submit_scenario(request).await,
        }
    }

    /// Commits the plain message and its queued turn before publishing live events.
    pub async fn submit_message(&self, message: UserMessage) -> Result<UserReceipt> {
        ensure!(
            !message.content.trim_start().starts_with('/'),
            "slash commands require the scenario route"
        );
        let execution = Transport::new(self.state)
            .handle(Task::UserMessage {
                request_uid: message.request_uid,
                thread_id: message.thread_id,
                content: message.content,
                cwd: message.cwd,
                attachments: message.attachments,
            })
            .await?;
        let turn = self.state.load_turn(&execution.request_uid).await?;
        Ok(UserReceipt { execution, turn })
    }

    /// Scenarios deliberately have a separate method and currently perform no writes.
    pub async fn submit_scenario(&self, _request: ScenarioRequest) -> Result<UserReceipt> {
        bail!("scenario route is not implemented")
    }
}
