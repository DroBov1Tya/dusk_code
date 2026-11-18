//! The mode a chat's step runs in. The setting is global; a chat takes it up when its
//! user writes to it, and keeps it for the turns the core starts there, such as for a
//! line from its team. An agent's thread has no mode of its own.

use std::sync::Arc;

use tracing::warn;

use super::Agent;
use crate::{catalog::modes::Modes, config::Config, runtime::thread::Conversation};

impl Agent {
    /// The settings for this thread's next step, with the chat's own mode laid over the
    /// setting's. A mode whose prompt is gone falls back to the setting's.
    pub(crate) async fn config_now(&self) -> Arc<Config> {
        let config = self.config.current();
        if self.depth != 0 {
            return config;
        }
        let mode = match self.state.chat_mode(&self.thread_id).await {
            Ok(Some(mode)) => mode,
            Ok(None) => return config,
            Err(error) => {
                warn!("thread {}: {error:#}", self.thread_id);
                return config;
            }
        };
        let known = Modes::new(&self.modes_dir).list().contains(&mode);
        if mode == config.mode() || !known {
            return config;
        }
        let mut own = (*config).clone();
        own.set_mode(mode);
        Arc::new(own)
    }

    /// A turn about to start in a chat: when the user wrote to it, the chat takes up the
    /// mode they wrote in. A setting that names no mode is not kept; a failed write keeps
    /// the chat's mode as it was, which never stops the turn.
    pub(super) async fn take_up_mode(&self, conversation: &Conversation) {
        if self.depth != 0 || !conversation.user_wrote() {
            return;
        }
        let mode = self.config.current().mode().to_owned();
        if !Modes::new(&self.modes_dir).list().contains(&mode) {
            return;
        }
        if let Err(error) = self.state.set_chat_mode(&self.thread_id, &mode).await {
            warn!("thread {}: {error:#}", self.thread_id);
        }
    }
}
