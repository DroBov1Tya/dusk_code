//! Words the team forwards from one thread to another, cleared of secrets first: a
//! member's or a program's words on their way to the lead, to a task's note or to a
//! client, and the lead's words on their way to a member. One scrubber does it, the one
//! every episode passes through (`project::episodes`): the values of the project's
//! variables and whatever is shaped like a key are taken out.

use tracing::warn;

use super::dispatch;
use crate::{project::episodes, runtime::Launcher, state::MAX_NOTE};

/// What stands in for a text that could not be cleaned. The text itself is dropped:
/// forwarding it unchecked could carry a secret.
const WITHHELD: &str = "(text withheld: the project's variables could not be read)";

/// `text` cleaned line by line, as the chat `lead`'s project asks.
pub(super) async fn text(launcher: &Launcher, lead: &str, text: &str) -> String {
    let cleaned = async {
        let folder = dispatch::folder(launcher, lead).await?;
        episodes::forwarded(&launcher.state, &folder, text).await
    };
    match cleaned.await {
        Ok(cleaned) => cleaned,
        Err(error) => {
            warn!("team of thread {lead}: a text was withheld: {error:#}");
            WITHHELD.into()
        }
    }
}

/// `words` cleaned and cut to what a task's note holds.
pub(super) async fn note(launcher: &Launcher, lead: &str, words: &str) -> String {
    let cleaned = text(launcher, lead, words).await;
    cleaned.chars().take(MAX_NOTE).collect()
}
