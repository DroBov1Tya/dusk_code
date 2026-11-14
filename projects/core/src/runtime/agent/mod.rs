//! One thread's agent: the loop of model calls and tool calls, compaction of its context,
//! and the inbox and mailboxes that carry messages to it; `mail` delivers messages between
//! agents and reports an agent's answer to its asker.

// Named after its primary item, `Agent`, as the style guide asks.
#[allow(clippy::module_inception)]
mod agent;
mod compaction;
pub mod generation;
mod inbox;
mod mail;
mod mailboxes;
mod rate;
mod repeats;
mod vision;

pub use agent::Agent;
pub use inbox::{Inbox, Input, Stop};
pub use mailboxes::{Mailbox, Mailboxes};
