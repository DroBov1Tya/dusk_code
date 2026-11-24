//! A goal's turns: after each turn of a chat with an active goal, the evaluator judges it,
//! and a goal not met starts the next turn with the evaluator's reason. There is no cap:
//! a user who wants a bound writes it into the goal, and the evaluator judges it. Each
//! verdict is stored in the chat as a `MessageRole::Goal` message the model never reads,
//! so a reopened transcript shows it.
//!
//! A turn is judged only once nothing runs in the background: while a job, an agent or
//! a question to the user is under way, the goal waits for its result and the turn it
//! starts.
//!
//! Stall rule: when `STALL_TURNS` turns in a row end without calling a tool, the agent is
//! only answering the evaluator, so no turn is started. The goal stays set, control goes
//! back to the user, and the goal goes on after their next message. Turns that do call
//! tools yet leave the files and test results as they were are `stall`'s: once
//! `limits.goal_stall_rounds` of them follow each other, the goal pauses.
//!
//! A failed turn: an error the provider may get over is tried again `RETRIES` times, a
//! spent limit or another failure pauses the goal, and one that no retry fixes clears it.

use std::sync::Arc;

use anyhow::Result;
use dusk_protocol::system::now_ms;

use super::command as goal;
use crate::{
    providers::ProviderErrorKind,
    runtime::{ThreadEvent, ThreadRunner, agent::Stop},
    state::{GoalEvaluation, GoalStatus, GoalVerdict},
};

/// Turns in a row without a tool call after which a goal stops starting turns.
pub const STALL_TURNS: u32 = 3;
/// Times a turn that failed on an error the provider may get over is tried again.
pub const RETRIES: u32 = 3;

impl ThreadRunner {
    /// Counts a turn of an active goal that called no tool toward the stall rule; one
    /// that called a tool, or a turn without an active goal, starts the count again.
    pub async fn count_quiet_turn(&mut self, turn_id: &str) -> Result<()> {
        if self.agent.active_goal().await?.is_none() {
            self.goal_quiet_turns = 0;
            return Ok(());
        }
        self.goal_quiet_turns = match self.agent.state.turn_called_tools(turn_id).await? {
            true => 0,
            false => self.goal_quiet_turns + 1,
        };
        Ok(())
    }

    /// Judges an active goal after its turn and starts the next turn when it is not met;
    /// `ended` is the turn that ran, when one did. Returns whether a turn was queued.
    pub async fn next_goal_round(&mut self, ended: Option<&str>) -> Result<bool> {
        let Some(goal) = self.agent.active_goal().await? else {
            self.goal_waiting = false;
            return Ok(false);
        };
        // A runner registers its report while holding this same lock. Checking
        // durable queued/running work under it closes the handoff to a new runner;
        // a finished report clears itself only after its results are stored.
        let mailboxes = Arc::clone(&self.mailboxes);
        let live = mailboxes.lock().await;
        let descendants = self
            .agent
            .state
            .descendant_threads(&self.agent.thread_id)
            .await?;
        let reporting = mailboxes.has_agent_reports(&descendants).await;
        let (pending, dependencies) = self
            .agent
            .state
            .goal_wake_state(&self.agent.thread_id)
            .await?;
        drop(live);
        if pending {
            // A result or user message won the race with start_turn: answer it, and
            // judge the turn that does.
            self.goal_waiting = true;
            self.take_in().await?;
            return Ok(true);
        }
        self.goal_waiting = reporting || dependencies;
        if self.goal_waiting {
            return Ok(false);
        }
        let judgment = match self.inbox.race(self.agent.judge_goal(&goal)).await {
            Ok(judgment) => judgment,
            Err(Stop::Cancelled) => {
                self.cancel_work().await?;
                return Ok(false);
            }
            // The user's message goes first; the turn it starts is judged.
            Err(Stop::Preempted) => {
                self.take_in().await?;
                return Ok(true);
            }
        };
        let state = &self.agent.state;
        let thread_id = &self.agent.thread_id;
        let Some(goal) = state.count_goal_turn(thread_id, &judgment.reason).await? else {
            return Ok(false);
        };
        let evaluation = GoalEvaluation {
            verdict: judgment.verdict,
            reason: judgment.reason.clone(),
            turn: goal.turns,
            tokens: goal.tokens,
            elapsed_ms: now_ms() - goal.started_at,
        };
        // Kept in the chat, so a reopened transcript still says how the goal went.
        state.add_goal_verdict(thread_id, &evaluation).await?;
        self.agent.emit(ThreadEvent::GoalEvaluated(evaluation));
        if judgment.verdict != GoalVerdict::NotMet {
            if judgment.verdict == GoalVerdict::Met {
                self.agent.record_met(&goal).await;
            }
            state.clear_goal(thread_id).await?;
            self.goal_quiet_turns = 0;
            self.agent.emit(ThreadEvent::Goal {
                goal: None,
                note: String::new(),
                started: None,
            });
            return Ok(false);
        }
        // A stop that came while the turn was judged.
        if goal.status != GoalStatus::Active {
            self.agent.emit(ThreadEvent::Goal {
                goal: Some(goal),
                note: String::new(),
                started: None,
            });
            return Ok(false);
        }
        if let Some(turn_id) = ended
            && let Some(reason) = self.round_stalled(turn_id).await?
        {
            self.goal_quiet_turns = 0;
            self.pause_goal(&reason).await?;
            return Ok(false);
        }
        if self.goal_quiet_turns >= STALL_TURNS {
            // The count starts again, so the user's next turn resumes the goal.
            self.goal_quiet_turns = 0;
            self.agent.emit(ThreadEvent::Goal {
                goal: Some(goal),
                note: format!(
                    "Goal stalled: {STALL_TURNS} turns in a row used no tool. \
                     It stays set and goes on after your next message."
                ),
                started: None,
            });
            return Ok(false);
        }
        if self.has_pending().await? {
            // The user sent something while the turn was judged, perhaps `/goal clear`:
            // it goes first, and the turn it starts is judged.
            self.goal_waiting = true;
            self.take_in().await?;
            return Ok(true);
        }
        let next = goal::continuation(&judgment.reason);
        state.add_pending(thread_id, &next).await?;
        self.agent.emit(ThreadEvent::Goal {
            goal: Some(goal),
            note: String::new(),
            started: Some(next),
        });
        self.take_in().await?;
        Ok(true)
    }

    /// After a turn of an active goal failed: tries it again when the provider may get
    /// over the error, pauses the goal when a limit is spent or the failure would repeat,
    /// and clears it when nothing the agent does can fix it. Returns whether a turn was
    /// queued.
    pub async fn goal_turn_failed(&mut self) -> Result<bool> {
        let Some(goal) = self.agent.active_goal().await? else {
            return Ok(false);
        };
        let state = &self.agent.state;
        let thread_id = &self.agent.thread_id;
        let failure = state.last_failure(thread_id).await?;
        let kind = failure
            .as_ref()
            .and_then(|failure| failure.provider)
            .map(|provider| provider.kind);
        let error = failure.map_or_else(|| "the turn failed".to_owned(), |f| f.text());
        use ProviderErrorKind::*;
        match kind {
            Some(Auth | Refused | TooLong) => {
                state.clear_goal(thread_id).await?;
                self.agent.emit(ThreadEvent::Goal {
                    goal: None,
                    note: format!(
                        "Goal cleared after an unrecoverable error: {error}. \
                         Run /goal again once it is fixed."
                    ),
                    started: None,
                });
                Ok(false)
            }
            Some(RateLimited | Quota) => {
                self.pause_goal(&format!("{error}; it goes on after your next message"))
                    .await?;
                Ok(false)
            }
            Some(Unreachable | TimedOut | Overloaded | Interrupted)
                if self.goal_retries < RETRIES =>
            {
                self.goal_retries += 1;
                let next = goal::retry(&error);
                state.add_pending(thread_id, &next).await?;
                self.agent.emit(ThreadEvent::Goal {
                    goal: Some(goal),
                    note: format!("Goal: trying again ({} of {RETRIES})", self.goal_retries),
                    started: Some(next),
                });
                self.take_in().await?;
                Ok(true)
            }
            Some(_) => {
                self.pause_goal(&format!("{error}, tried {RETRIES} more times"))
                    .await?;
                Ok(false)
            }
            None => {
                self.pause_goal("the last turn failed").await?;
                Ok(false)
            }
        }
    }

    /// Pauses the chat's goal, if it has one, and says why.
    pub async fn pause_goal(&mut self, reason: &str) -> Result<()> {
        self.goal_retries = 0;
        if self.agent.depth > 0
            || self
                .agent
                .state
                .goal(&self.agent.thread_id)
                .await?
                .is_none()
        {
            return Ok(());
        }
        let goal = self.agent.state.set_goal_status(
            &self.agent.thread_id,
            GoalStatus::Paused,
            Some(reason),
        );
        let goal = goal.await?;
        self.agent.emit(ThreadEvent::Goal {
            goal,
            note: format!("Goal paused: {reason}"),
            started: None,
        });
        Ok(())
    }
}
