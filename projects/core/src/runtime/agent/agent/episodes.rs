//! Writing episodes down from what a turn saw (`limits.episodes`): a loop a pass ended,
//! a goal the evaluator found met, and the model's lesson line. Every failure here is
//! logged by `project::episodes` and dropped: an episode never fails a turn.

use dusk_protocol::thread::EpisodeKind;

use super::Agent;
use crate::project::episodes::{self, Facts, Origin};
use crate::providers::ToolCall;
use crate::runtime::{ThreadEvent, agent::repeats::Solved};
use crate::state::{ChangeQuery, Goal};

/// Ledger rows read for the files a loop changed; the episode names four of them.
const MAX_CHANGES: usize = 64;
/// The tool a verified episode is filed under: its checks are commands.
const CHECK_TOOL: &str = "run_command";

impl Agent {
    /// Writes down the loop `call` ended in `turn_id`; whether an episode was written.
    pub(super) async fn record_fixed(
        &self,
        turn_id: &str,
        call: &ToolCall,
        solved: &Solved,
    ) -> bool {
        let changed = self.changed_since(turn_id, &call.id, solved.since).await;
        let facts = Facts::Fixed {
            key: &solved.key,
            problem: &solved.problem,
            attempts: &solved.attempts,
            passed: &solved.passed,
            changed: &changed,
            lesson: None,
        };
        self.record_episode(Some(turn_id), &call.name, &facts).await
    }

    /// Writes down a goal the evaluator found met, with the commands that passed while
    /// the agent worked on it.
    pub async fn record_met(&self, goal: &Goal) {
        if !self.config.current().limits().episodes {
            return;
        }
        let commands = self
            .state
            .goal_commands_since(&self.thread_id, goal.started_at);
        let commands = match commands.await {
            Ok(commands) => commands,
            Err(error) => {
                tracing::warn!("cannot read the goal's commands: {error:#}");
                return;
            }
        };
        let facts = Facts::Verified {
            objective: &goal.text,
            commands: &commands,
        };
        self.record_episode(None, CHECK_TOOL, &facts).await;
    }

    /// Puts the lesson line of the turn's answer on the turn's episodes, when a result of
    /// the turn asked for one.
    pub(super) async fn store_lesson(&self, turn_id: &str, answer: &str) {
        let limits = self.config.current().limits();
        let asked = self.repeats.lock().lesson_asked(turn_id);
        if !(limits.episodes && limits.episode_lesson && asked) {
            return;
        }
        let Some(lesson) = crate::runtime::agent::repeats::lesson(answer) else {
            return;
        };
        // Logged where it failed.
        let _ = episodes::set_lesson(&self.state, self.scope.cwd(), turn_id, lesson).await;
    }

    /// Records the episode and tells `recall` and the clients; whether it was written.
    async fn record_episode(&self, turn_id: Option<&str>, tool: &str, facts: &Facts<'_>) -> bool {
        let project = self.scope.cwd();
        let model = self.model.current().map(|choice| choice.model.id);
        let origin = Origin {
            project,
            thread_id: Some(&self.thread_id),
            turn_id,
            tool,
            model: model.as_deref().unwrap_or_default(),
        };
        // `record` logs its error; `None` is a refusal, such as a folder not saved.
        let Ok(Some(_)) = episodes::record(&self.state, &origin, facts).await else {
            return false;
        };
        if let Some(name) = project.to_str() {
            self.state.note_episode(name);
        }
        let kind = match facts {
            Facts::Fixed { .. } => EpisodeKind::Fixed,
            Facts::Verified { .. } => EpisodeKind::Verified,
        };
        self.emit(ThreadEvent::EpisodeRecorded { kind });
        true
    }

    /// The files the ledger recorded in this turn since `since`, oldest first and each
    /// once, leaving out what the call `passed` changed itself.
    async fn changed_since(&self, turn_id: &str, passed: &str, since: i64) -> Vec<String> {
        let project = self.scope.cwd().to_string_lossy();
        let query = ChangeQuery {
            project: &project,
            thread_id: Some(&self.thread_id),
            path: None,
            since: Some(since),
            limit: MAX_CHANGES,
        };
        let changes = match self.state.changes(&query).await {
            Ok(changes) => changes,
            Err(error) => {
                tracing::warn!("cannot read the changes for an episode: {error:#}");
                return Vec::new();
            }
        };
        let mut paths: Vec<String> = Vec::new();
        for change in changes.into_iter().rev() {
            let ours = change.turn_id.as_deref() == Some(turn_id)
                && change.call_id.as_deref() != Some(passed);
            if ours && !paths.contains(&change.path) {
                paths.push(change.path);
            }
        }
        paths
    }
}
