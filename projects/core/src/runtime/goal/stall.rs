//! A goal whose rounds stop changing anything. After each round the state of the files
//! the chat and its agents changed (by the change ledger) and the test results of the
//! round's last turn, error lines included, are hashed and stored with the goal; once
//! `limits.goal_stall_rounds` measured rounds in a row left both as they were, another
//! round would only do the same, and the goal pauses for the user with what stayed the
//! same.
//!
//! A round is measured by what it did itself, and only where the ledger can be trusted.
//! A round that changed no file and ran no test (research, a question, a look at a job)
//! leaves the count as it was, and so does one that may have written where the ledger
//! does not look (a background job, a subagent, an MCP tool), or one in a chat the
//! ledger never recorded a change of (recording off, or a project too large to watch):
//! there, the same test results say nothing about the code.

use anyhow::Result;
use dusk_protocol::text::clip_chars;
use sha2::{Digest, Sha256};

use crate::runtime::{ThreadRunner, agent::TestOutcome};

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
        let round = self.agent.repeats.lock().round(turn_id);
        if round.unseen_writes {
            return Ok(None);
        }
        let state = &self.agent.state;
        let mut threads = state.descendant_threads(&self.agent.thread_id).await?;
        threads.push(self.agent.thread_id.clone());
        let files = state.files_state(&threads).await?;
        if files.is_empty() {
            return Ok(None);
        }
        if round.tests.is_empty() && !state.changed_in_turn(&threads, turn_id).await? {
            return Ok(None);
        }
        let Some(same) = state
            .note_goal_round(&self.agent.thread_id, &fingerprint(&files, &round.tests))
            .await?
        else {
            return Ok(None);
        };
        let unchanged = same.saturating_sub(1);
        if unchanged < limits.goal_stall_rounds {
            return Ok(None);
        }
        Ok(Some(reason(unchanged, &round.tests)))
    }
}

/// The files' states and the test results, hashed the same way by every build: it is
/// stored. Each field is length-prefixed, so no two lists hash alike by their joins.
fn fingerprint(files: &[(String, String)], tests: &[TestOutcome]) -> String {
    let mut hasher = Sha256::new();
    let mut field = |text: &str| {
        hasher.update((text.len() as u64).to_le_bytes());
        hasher.update(text.as_bytes());
    };
    for (path, state) in files {
        field(path);
        field(state);
    }
    field("tests");
    for test in tests {
        field(&test.shown);
        field(&test.error);
    }
    let digest = hasher.finalize();
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn reason(rounds: u32, tests: &[TestOutcome]) -> String {
    let tests = match tests {
        [] => String::new(),
        tests => {
            let shown: Vec<String> = tests
                .iter()
                .take(MAX_SHOWN_TESTS)
                .map(|test| clip_chars(&test.shown, MAX_TEST_CHARS, "…"))
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
