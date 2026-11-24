//! A goal's rounds: after each turn of a chat with an active goal, the runner checks
//! the work against the goal's criteria when the model asked for that, and otherwise
//! queues the next round. Rounds are counted for display only; a user who wants a bound
//! writes it into the goal.
//!
//! Stall rule: when `STALL_TURNS` turns in a row end without calling a tool, the agent is
//! only talking, so no round is queued. The goal stays set, control goes back to the user,
//! and the rounds go on after the next turn the user starts.

use std::sync::Arc;

use anyhow::Result;

use super::action as goal;
use crate::{
    runtime::{ThreadEvent, ThreadRunner},
    state::GoalStatus,
};

/// Turns in a row without a tool call after which a goal stops queueing rounds.
pub const STALL_TURNS: u32 = 3;

impl ThreadRunner {
    /// Counts a turn that called no tool toward the stall rule; one that called a tool
    /// starts the count again.
    pub async fn count_quiet_turn(&mut self, turn_id: &str) -> Result<()> {
        if self.agent.depth > 0 {
            return Ok(());
        }
        self.goal_quiet_turns = match self.agent.state.turn_called_tools(turn_id).await? {
            true => 0,
            false => self.goal_quiet_turns + 1,
        };
        Ok(())
    }

    pub async fn verification_pending(&self) -> Result<bool> {
        Ok(self.agent.depth == 0
            && self
                .agent
                .state
                .goal(&self.agent.thread_id)
                .await?
                .is_some_and(|g| {
                    g.approved && g.status == GoalStatus::Active && g.verification == "pending"
                }))
    }

    /// Queues the next round of an active goal. Returns whether a round was queued.
    pub async fn next_goal_round(&mut self) -> Result<bool> {
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
        if !pending && !dependencies && !reporting && goal.verification == "pending" {
            if let Some(reviewed) =
                super::verification::verify(&self.agent.state, &self.agent.thread_id).await?
            {
                // `verify` moves a goal from checking to passed only here, once.
                if reviewed.verification == "passed" {
                    self.agent.record_verified(&reviewed).await;
                }
                let note = goal::describe(&reviewed);
                self.agent.emit(ThreadEvent::Goal {
                    goal: Some(reviewed.clone()),
                    note,
                    started: None,
                });
                if reviewed.status == GoalStatus::Done || reviewed.verification == "needs_user" {
                    self.goal_waiting = false;
                    return Ok(false);
                }
            } else {
                self.goal_waiting = true;
                return Ok(false);
            }
        }
        drop(live);
        if pending {
            // A result or user message won the race with start_turn: answer it
            // before creating another goal message or consuming a round.
            self.goal_waiting = true;
            self.take_in().await?;
            return Ok(true);
        }
        self.goal_waiting = reporting || dependencies;
        if self.goal_waiting {
            return Ok(false);
        }
        if goal.verification == "needs_user" {
            return Ok(false);
        }
        if self.goal_quiet_turns >= STALL_TURNS {
            // The count starts again, so the user's next turn resumes the rounds.
            self.goal_quiet_turns = 0;
            self.agent.emit(ThreadEvent::Goal {
                goal: Some(goal),
                note: format!(
                    "goal stalled: {STALL_TURNS} turns in a row used no tool; \
                     it stays set and goes on after your next message"
                ),
                started: None,
            });
            return Ok(false);
        }
        let state = &self.agent.state;
        let Some(goal) = state.count_goal_round(&self.agent.thread_id).await? else {
            return Ok(false);
        };
        let kickoff = goal::kickoff(&goal);
        state.add_pending(&self.agent.thread_id, &kickoff).await?;
        let note = format!("goal round {}", goal.rounds);
        self.agent.emit(ThreadEvent::Goal {
            goal: Some(goal),
            note,
            started: Some(kickoff),
        });
        self.take_in().await?;
        Ok(true)
    }

    pub async fn pause_goal(&self, reason: &str) -> Result<()> {
        if self.agent.active_goal().await?.is_none() {
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
            note: format!("goal paused: {reason}"),
            started: None,
        });
        Ok(())
    }
}
