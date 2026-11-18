//! Command output sent shaped (`limits.shape_output`, `docs/context-life.md` "Shaped
//! output"): as a result is stored, the text the model is sent in its place is decided
//! once and kept beside it. The model gets the tool's shaped output, the notes added
//! after the call, and a last line that says what was left out and how to read it.

use tracing::warn;

use super::{Agent, stubs::left_out_note};
use crate::{
    runtime::ThreadEvent,
    state::{Message, saved_tokens},
    tools::Shaped,
};

/// Less saved than this is not worth the note, nor a read it may prompt.
const MIN_SAVED_TOKENS: u64 = 100;

impl Agent {
    /// Stores the shaped text of `stored`: `shaped` output, then `notes`, the text added
    /// to the tool's output after the call. A failure is logged and the result is sent
    /// whole, as it is with the switch off.
    pub(super) async fn send_shaped(&self, stored: &Message, shaped: Shaped, notes: &str) {
        let note = left_out_note(stored.sequence, shaped.left_out, &shaped.what);
        let text = format!("{}{notes}\n{note}", shaped.text);
        if saved_tokens(stored.content_tokens, &text) < MIN_SAVED_TOKENS {
            return;
        }
        let added = self
            .state
            .add_shaped(&self.thread_id, &stored.message_id, &text);
        match added.await {
            Ok(saved_tokens) => self.emit(ThreadEvent::ResultShaped { saved_tokens }),
            Err(error) => warn!(
                "thread {}: the output is sent whole: {error:#}",
                self.thread_id
            ),
        }
    }
}
