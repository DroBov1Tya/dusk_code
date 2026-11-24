//! A goal whose rounds stop changing anything. After each round the state of the files
//! the chat and its agents changed (by the change ledger) and the test results of the
//! round's last turn are hashed and stored with the goal; once `limits.goal_stall_rounds`
//! rounds in a row left both as they were, another round would only do the same, and the
//! goal pauses for the user with what stayed the same.
//!
//! A round with nothing to compare by, no file changed in the goal's chat and no test
//! run, never pauses it: research and questions are not measured here.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use anyhow::Result;
use dusk_protocol::text::clip_chars;

use crate::runtime::ThreadRunner;

/// Test results shown in the reason a goal paused, and the characters of each.
const MAX_SHOWN_TESTS: usize = 3;
const MAX_TEST_CHARS: usize = 160;

impl ThreadRunner {
    /// Notes how the round that ended with the turn `turn_id` left the work; why the goal
    /// must pause when the rounds stopped changing it.
    pub(super) async fn round_stalled(&self, turn_id: &str) -> Result<Option<String>> {
        let limits = self.agent.config.current().limits();
        if !(limits.goal_stall && limits.record_changes) {
            return Ok(None);
        }
        let state = &self.agent.state;
        let mut threads = state.descendant_threads(&self.agent.thread_id).await?;
        threads.push(self.agent.thread_id.clone());
        let files = state.files_state(&threads).await?;
        let tests = self.agent.repeats.lock().test_results(turn_id);
        let mut hasher = DefaultHasher::new();
        (&files, &tests).hash(&mut hasher);
        let fingerprint = format!("{:016x}", hasher.finish());
        let Some(same) = state
            .note_goal_round(&self.agent.thread_id, &fingerprint)
            .await?
        else {
            return Ok(None);
        };
        let unchanged = same.saturating_sub(1);
        if unchanged < limits.goal_stall_rounds || (files.is_empty() && tests.is_empty()) {
            return Ok(None);
        }
        Ok(Some(reason(unchanged, &tests)))
    }
}

fn reason(rounds: u32, tests: &[String]) -> String {
    let tests = match tests {
        [] => String::new(),
        tests => {
            let shown: Vec<String> = tests
                .iter()
                .take(MAX_SHOWN_TESTS)
                .map(|test| clip_chars(test, MAX_TEST_CHARS, "…"))
                .collect();
            let mut shown = shown.join("; ");
            if tests.len() > MAX_SHOWN_TESTS {
                shown.push_str("; ...");
            }
            format!(", tests: {shown}")
        }
    };
    format!("the last {rounds} rounds changed nothing: the same files{tests}")
}
