//! What a client holds of a running thread: the input that feeds it and the output that tells it.

use crate::runtime::ThreadEvent;
use crate::runtime::agent::Input;
use crate::runtime::model_choice::ModelChoice;
use crate::{
    config::live::LiveConfig,
    state::{State, estimate_tokens},
};
use anyhow::{Result, ensure};
use std::sync::Arc;
use tokio::sync::mpsc;

/// The client's side for sending. Dropping every clone ends the thread once the queue is done.
#[derive(Clone)]
pub struct ThreadInput {
    pub(super) state: Arc<State>,
    pub(super) config: Arc<LiveConfig>,
    pub(super) model: Arc<ModelChoice>,
    /// Weak, so the client's output ends when the runner does and the client can tell.
    pub(super) events: mpsc::WeakUnboundedSender<ThreadEvent>,
    pub(super) thread_id: String,
    pub(super) inputs: mpsc::UnboundedSender<Input>,
}

/// The client's side for receiving.
pub struct ThreadOutput {
    pub(super) events: mpsc::UnboundedReceiver<ThreadEvent>,
}

impl ThreadInput {
    /// Queues a message; it is answered after the running turn, together with other
    /// queued ones. Once this returns, the message is stored.
    pub async fn send(&self, text: impl Into<String>) -> Result<()> {
        self.deliver(text.into(), Input::Sent).await
    }

    /// Queues a message and stops the running turn, so the agent starts on it at once.
    pub async fn send_now(&self, text: impl Into<String>) -> Result<()> {
        self.deliver(text.into(), Input::SentNow).await
    }

    /// Stops the running turn; queued messages wait for the next one.
    pub async fn cancel(&self) -> Result<()> {
        // Serializes cancellation with the verification commit. Once this returns,
        // an active goal cannot complete on an earlier request.
        sqlx::query("UPDATE goals SET status = 'paused', note = 'stopped by the user',
            verification = CASE WHEN verification IN ('pending', 'checking') THEN 'gaps' ELSE verification END,
            verification_note = CASE WHEN verification IN ('pending', 'checking') THEN 'Verification cancelled; no pass was recorded.' ELSE verification_note END
            WHERE thread_id = ? AND status = 'active'")
            .bind(&self.thread_id).execute(&self.state.pool).await?;
        ensure!(self.inputs.send(Input::Cancel).is_ok(), "thread has ended");
        Ok(())
    }

    async fn deliver(&self, text: String, input: Input) -> Result<()> {
        ensure!(!self.inputs.is_closed(), "thread has ended");
        // A message that alone is past the input limit could never be answered.
        let config = self.config.current();
        let limit = self
            .model
            .current()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(&config)
            });
        let tokens = estimate_tokens(&text);
        ensure!(
            tokens <= limit,
            "the message is about {tokens} tokens, past the input limit of {limit}"
        );
        if self.model.current().is_none() {
            let message = self.state.add_local_unsent(&self.thread_id, &text).await?;
            if let Some(events) = self.events.upgrade() {
                let _ = events.send(ThreadEvent::LocalUnsent(message));
                let _ = events.send(ThreadEvent::ProviderUnavailable);
            }
            return Ok(());
        }
        self.state.add_pending(&self.thread_id, &text).await?;
        // Stored either way: a thread that has just ended takes it in on its next start.
        let _ = self.inputs.send(input);
        Ok(())
    }
}

impl ThreadOutput {
    /// `None` once the thread has ended.
    pub async fn next_event(&mut self) -> Option<ThreadEvent> {
        self.events.recv().await
    }
}
