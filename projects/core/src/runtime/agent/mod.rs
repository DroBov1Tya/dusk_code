//! One thread's agent: the loop of model calls and tool calls, compaction of its context,
//! and the inbox and mailboxes that carry messages to it.

// Named after its primary item, `Agent`, as the style guide asks.
#[allow(clippy::module_inception)]
mod agent;
mod compaction;
pub mod generation;
mod inbox;
mod mailboxes;
mod rate;
mod repeats;
mod vision;

pub use agent::Agent;
pub use inbox::{Inbox, Input, Stop};
pub use mailboxes::{Mailbox, Mailboxes};
