//! `/compact [instructions]`: summarizes the thread so far to free the context. Text after
//! the command goes to the model as extra instructions for this summary.

use crate::runtime::agent::{Agent, Inbox};

pub async fn run(agent: &Agent, instructions: Option<&str>, inbox: &mut Inbox) {
    // A stop only ends the compaction; the client decides what comes next.
    let _ = agent.compact(instructions, None, inbox).await;
}
