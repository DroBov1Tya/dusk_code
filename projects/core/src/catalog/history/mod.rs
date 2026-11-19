//! What changed in skills during turns, kept to undo it. Rows of temporary chats stay in
//! the main database too: the skill file outlives the chat.

mod diff;
mod text;
mod watch;

pub use diff::{counts, diff};
pub use text::{MAX_REASON_CHARS, plain_line};
pub use watch::{Baseline, Watch};
