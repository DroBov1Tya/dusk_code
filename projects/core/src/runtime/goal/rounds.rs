//! A goal's rounds: after each turn of a chat with an active goal, the runner checks
//! the work against the goal's criteria when the model asked for that, and otherwise
//! queues the next round until the rounds run out.

use std::sync::Arc;

use anyhow::Result;

use super::action as goal;
use crate::{
    runtime::{ThreadEvent, ThreadRunner},
    state::GoalStatus,
};

impl ThreadRunner {
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

    /// Queues the next round of an active goal, or pauses it when the rounds are spent.
    /// Returns whether a round was queued.
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
        let rounds = self.agent.config.current().limits().max_goal_rounds;
        if goal.rounds >= rounds {
            self.pause_goal(&format!("all {rounds} rounds used"))
                .await?;
            return Ok(false);
        }
        let state = &self.agent.state;
        let Some(goal) = state.count_goal_round(&self.agent.thread_id).await? else {
            return Ok(false);
        };
        let kickoff = goal::kickoff(&goal, rounds);
        state.add_pending(&self.agent.thread_id, &kickoff).await?;
        let note = format!("goal round {} of {rounds}", goal.rounds);
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
