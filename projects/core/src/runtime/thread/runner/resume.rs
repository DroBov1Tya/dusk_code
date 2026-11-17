//! The first thing a runner does when a stopped process left its thread's turn running:
//! goes on with it, or asks the user and waits for the answer.

use anyhow::Result;
use dusk_protocol::tools::Question;

use super::ThreadRunner;
use crate::runtime::ThreadEvent;
use crate::runtime::agent::{Decision, Input, Recovered};
use crate::state::Turn;

/// How a thread goes on after what a restart found.
pub(super) enum Restart {
    /// This turn goes on before anything else.
    GoOn(Turn),
    /// No turn is running: the thread takes in what was sent, as always.
    Idle,
    /// Every client went away while the user was asked; the turn still waits.
    Closed,
}

impl ThreadRunner {
    pub(super) async fn take_up(&mut self, recovered: Recovered) -> Result<Restart> {
        match recovered {
            Recovered::Nothing => Ok(Restart::Idle),
            Recovered::Resume(turn) => Ok(Restart::GoOn(turn)),
            Recovered::Decide(turn, question) => self.ask(turn, question).await,
        }
    }

    /// Asks the user how the turn goes on and waits for the answer. Only the user's
    /// messages are read meanwhile: another agent's would come between the step's calls
    /// and their results, so they wait stored. So does what the user sent before the
    /// question, which this turn would not have answered either; an answer stored before
    /// a restart still counts.
    async fn ask(&mut self, turn: Turn, question: Question) -> Result<Restart> {
        self.agent.emit(ThreadEvent::TurnWaiting(turn.clone()));
        self.agent.emit(ThreadEvent::Question(question.clone()));
        let state = std::sync::Arc::clone(&self.agent.state);
        let thread_id = self.agent.thread_id.clone();
        let earlier = state.pending_messages(&thread_id).await?;
        let asked_after = earlier.iter().map(|message| message.pending_id).max();
        loop {
            let pending = state.pending_messages(&thread_id).await?;
            let said = pending.into_iter().find_map(|message| {
                if message.from_thread_id.is_some() || message.from_job_id.is_some() {
                    return None;
                }
                match Decision::of(&question, &message.content) {
                    Some(decision) => Some((message, Some(decision))),
                    None if Some(message.pending_id) > asked_after => Some((message, None)),
                    None => None,
                }
            });
            match said {
                Some((message, Some(decision))) => {
                    return match self.agent.decide(&turn, &message, decision).await? {
                        Some(turn) => Ok(Restart::GoOn(turn)),
                        None => {
                            self.cancel_work().await?;
                            Ok(Restart::Idle)
                        }
                    };
                }
                // Anything else the user says stops the turn, and is then taken in as
                // any message is.
                Some((_, None)) => {
                    self.agent.stop_unfinished(None).await?;
                    return Ok(Restart::Idle);
                }
                None => {}
            }
            match self.inbox.wait().await {
                None => return Ok(Restart::Closed),
                Some(Input::Cancel) => {
                    self.agent.stop_unfinished(None).await?;
                    self.cancel_work().await?;
                    return Ok(Restart::Idle);
                }
                Some(_) => {}
            }
        }
    }
}
