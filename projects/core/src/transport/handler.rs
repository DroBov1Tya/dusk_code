//! In-process task handler. No scheduling or model provider is invoked.

use anyhow::Result;

use crate::{Execution, RejectionCode, Task, state::State};

pub struct Transport<'a> {
    state: &'a State,
}

impl<'a> Transport<'a> {
    pub fn new(state: &'a State) -> Self {
        Self { state }
    }

    /// Commits an accepted or rejected receipt before publishing its event.
    /// The caller retains content if it needs to retry or execute the request later.
    /// Invalid UIDs and storage failures return an error without a receipt.
    pub async fn handle(&self, task: Task) -> Result<Execution> {
        match task {
            Task::UserMessage {
                request_uid,
                thread_id,
                content,
            } => {
                let rejection = if content.trim().is_empty() {
                    Some(RejectionCode::EmptyMessage)
                } else if thread_id.trim().is_empty() {
                    Some(RejectionCode::EmptyThread)
                } else {
                    None
                };
                self.state
                    .receive_request(&request_uid, &thread_id, rejection)
                    .await
            }
        }
    }
}
