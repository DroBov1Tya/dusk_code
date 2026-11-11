//! Pictures in the context: attaching them, asking whether the model sees them.

use super::Agent;
use crate::runtime::{ThreadEvent, thread::Conversation};
use crate::{
    message::sources::{self},
    providers::{ChatMessage, ToolSpec},
    state::MessageRole,
};
use tracing::{info, warn};

impl Agent {
    /// The pictures of the newest step's tool results, such as a screenshot, go with
    /// them. Older ones stay out: each would cost the context again in every request,
    /// and the model looks again when it needs to.
    pub(super) fn attach_tool_pictures(
        &self,
        conversation: &Conversation,
        messages: &mut [ChatMessage],
    ) {
        let Some(newest) = conversation
            .messages
            .iter()
            .rposition(|message| message.role == MessageRole::Assistant)
        else {
            return;
        };
        for message in &conversation.messages[newest + 1..] {
            let Some(call) = message.tool_call_id.as_ref() else {
                continue;
            };
            let pictures: Vec<_> = message
                .artifacts
                .iter()
                .filter(|artifact| artifact.for_model)
                .filter_map(|artifact| self.artifacts.read(artifact).ok())
                .filter_map(|bytes| sources::picture(&bytes))
                .collect();
            if pictures.is_empty() {
                continue;
            }
            // From the end: a server may reuse call IDs across steps.
            let slot = messages.iter_mut().rev().find_map(|chat| match chat {
                ChatMessage::Tool {
                    call_id, pictures, ..
                } if call_id == call => Some(pictures),
                _ => None,
            });
            if let Some(slot) = slot {
                *slot = pictures;
            }
        }
    }

    /// Whether the current model sees pictures: asked once, then remembered. A model
    /// that refused pictures does not.
    pub(super) async fn sees_pictures(&self) -> bool {
        let Some(choice) = self.model.current() else {
            return false;
        };
        if !self.takes_pictures() {
            return false;
        }
        let name = choice.name();
        if let Some(sees) = self.model.vision(&name) {
            return sees;
        }
        if !self.model.may_ask_vision(&name) {
            return false;
        }
        match choice.sees_pictures().await {
            Some(sees) => {
                info!("model {name} sees pictures: {sees}");
                self.model.set_vision(name, sees);
                sees
            }
            None => {
                self.model.vision_failed(name);
                false
            }
        }
    }

    pub(super) fn takes_pictures(&self) -> bool {
        let refused = self.pictures_refused.lock();
        let current = self.model.current();
        current.is_none_or(|choice| refused.as_ref() != Some(&choice.model.id))
    }

    /// Takes the pictures out of the request, and sends none to this model from now on.
    pub(super) fn refuse_pictures(&self, model: &str, messages: &mut [ChatMessage]) {
        *self.pictures_refused.lock() = Some(model.to_owned());
        warn!("thread {} model {model} refused pictures", self.thread_id);
        self.emit(ThreadEvent::PicturesRefused {
            model: model.to_owned(),
        });
        for message in messages {
            match message {
                ChatMessage::UserWithPictures { text, .. } => {
                    let said = format!("{text}[this model refused the pictures named above]\n");
                    *message = ChatMessage::User(said);
                }
                ChatMessage::Tool {
                    content, pictures, ..
                } if !pictures.is_empty() => {
                    pictures.clear();
                    content.push_str("\n[this model refused the picture of this result]");
                }
                _ => {}
            }
        }
    }

    /// Reports a context past the input limit to the client and returns why.
    pub(super) fn context_too_large(
        &self,
        conversation: &Conversation,
        specs: &[ToolSpec],
        attached: u64,
    ) -> Option<String> {
        let tokens = conversation.tokens(specs) + attached;
        let limit = Some(self.input_limit()).filter(|&limit| tokens > limit)?;
        self.emit(ThreadEvent::ContextTooLarge { tokens, limit });
        Some(format!(
            "the context is about {tokens} tokens, past the input limit of {limit}"
        ))
    }
}
