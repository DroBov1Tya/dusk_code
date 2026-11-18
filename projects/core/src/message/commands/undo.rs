//! `/undo`: lists the user's messages to go back to; `/undo <n>` takes the thread back
//! to before message `n`, which a client then offers to edit and send again.

use crate::runtime::{ThreadEvent, agent::Agent};

pub async fn run(agent: &Agent, sequence: u64) {
    match agent.state.undo_from(&agent.thread_id, sequence).await {
        Ok(text) => {
            agent.emit(ThreadEvent::Undone { text });
            // The pins of the messages that went, went with them.
            match agent.state.pins(&agent.thread_id).await {
                Ok(pins) => agent.emit(ThreadEvent::Pins(pins)),
                Err(error) => tracing::warn!("cannot read the pins after /undo: {error:#}"),
            }
            agent.report_tokens().await;
        }
        Err(error) => agent.emit(ThreadEvent::Rejected {
            reason: format!("{error:#}"),
        }),
    }
}
