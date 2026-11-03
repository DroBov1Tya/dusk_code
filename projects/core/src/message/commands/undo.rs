//! `/undo`: lists the user's messages to go back to; `/undo <n>` takes the thread back
//! to before message `n`, which a client then offers to edit and send again.

use crate::runtime::{ThreadEvent, agent::Agent};

pub async fn run(agent: &Agent, sequence: u64) {
    match agent.state.undo_from(&agent.thread_id, sequence).await {
        Ok(text) => {
            agent.emit(ThreadEvent::Undone { text });
            agent.report_tokens().await;
        }
        Err(error) => agent.emit(ThreadEvent::Rejected {
            reason: format!("{error:#}"),
        }),
    }
}
