//! The user-facing entry point: accepts requests, without exposing state setters.

use anyhow::{Result, bail, ensure};

use crate::state::{
    Execution, Message, MessageAttachment, MessageInput, MessageRole, RejectionCode, State,
};

pub struct UserReceiver<'a> {
    state: &'a State,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum UserRequest {
    Message(UserMessage),
    Scenario(ScenarioRequest),
}

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
pub struct UserReceipt {
    pub execution: Execution,
    /// The stored message, now queued for the agent; `None` when the request was rejected.
    pub message: Option<Message>,
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

    /// Commits the receipt and the message in one transaction, then publishes live events.
    /// Invalid input gets a rejected receipt; an invalid UID, context or unavailable storage
    /// is an error without a receipt.
    pub async fn submit_message(&self, message: UserMessage) -> Result<UserReceipt> {
        ensure!(
            !message.content.trim_start().starts_with('/'),
            "slash commands require the scenario route"
        );
        let rejection = rejection(&message);
        let (execution, message) = self
            .state
            .receive_request(
                &message.request_uid,
                MessageInput {
                    thread_id: message.thread_id,
                    job_id: None,
                    role: MessageRole::User,
                    agent_id: None,
                    content: message.content,
                    reasoning: None,
                    cwd: message.cwd,
                    attachments: message.attachments,
                },
                rejection,
            )
            .await?;
        Ok(UserReceipt { execution, message })
    }

    /// Scenarios deliberately have a separate method and currently perform no writes.
    pub async fn submit_scenario(&self, _request: ScenarioRequest) -> Result<UserReceipt> {
        bail!("scenario route is not implemented")
    }
}

/// Unknown threads are detected by the database in the same statement that records the receipt.
fn rejection(message: &UserMessage) -> Option<RejectionCode> {
    if message.content.trim().is_empty() && message.attachments.is_empty() {
        Some(RejectionCode::EmptyMessage)
    } else if message.thread_id.trim().is_empty() {
        Some(RejectionCode::EmptyThread)
    } else {
        None
    }
}
