//! In-process task handler. No scheduling or model provider is invoked.

use anyhow::Result;

use crate::{
    Execution, RejectionCode, Task,
    state::{MessageInput, MessageRole, State},
};

pub struct Transport<'a> {
    state: &'a State,
}

impl<'a> Transport<'a> {
    pub fn new(state: &'a State) -> Self {
        Self { state }
    }

    /// Commits an accepted or rejected receipt before publishing its event.
    /// Accepted content and its context are committed in the same transaction.
    /// Invalid UIDs/context and storage failures return an error without a receipt.
    pub async fn handle(&self, task: Task) -> Result<Execution> {
        match task {
            Task::UserMessage {
                request_uid,
                thread_id,
                content,
                cwd,
                attachments,
            } => {
                let rejection = if content.trim().is_empty() && attachments.is_empty() {
                    Some(RejectionCode::EmptyMessage)
                } else if thread_id.trim().is_empty() {
                    Some(RejectionCode::EmptyThread)
                } else {
                    None
                };
                self.state
                    .receive_request(
                        &request_uid,
                        MessageInput {
                            thread_id,
                            job_id: None,
                            role: MessageRole::User,
                            agent_id: None,
                            content,
                            cwd,
                            attachments,
                        },
                        rejection,
                    )
                    .await
            }
        }
    }
}
