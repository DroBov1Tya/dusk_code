//! The Overview of a thread, counted from what the core keeps: tool calls from
//! `tool_executions`, changes from the ledger, and the event log from its turns and team.

use anyhow::{Context, Result};
use dusk_protocol::thread::{
    Activity, ChangeSum, ChangedPath, EVENTS_PAGE, LogEntry, LogKind, OVERVIEW_LOG, OVERVIEW_PATHS,
    OVERVIEW_TOOLS, Overview, ToolRuns, duration_words,
};

use super::State;

impl State {
    pub async fn overview(&self, thread_id: &str) -> Result<Overview> {
        Ok(Overview {
            activity: self.activity(thread_id).await?,
            changes: self.change_sum(thread_id).await?,
            log: self.events(thread_id, None, OVERVIEW_LOG as u32).await?,
        })
    }

    /// How often each tool ran in the thread, most used first, and the agents it asked.
    pub async fn activity(&self, thread_id: &str) -> Result<Activity> {
        let counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT tool_name, COUNT(*) AS runs FROM tool_executions WHERE thread_id = ?
             GROUP BY tool_name ORDER BY runs DESC, tool_name",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("count the tool runs")?;
        let agents: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE parent_thread_id = ?")
                .bind(thread_id)
                .fetch_one(&self.pool)
                .await
                .context("count the agents asked")?;
        let by_tool: Vec<ToolRuns> = counts
            .into_iter()
            .map(|(name, runs)| ToolRuns {
                name,
                runs: runs.unsigned_abs(),
            })
            .collect();
        Ok(Activity {
            tool_runs: by_tool.iter().map(|tool| tool.runs).sum(),
            by_tool: by_tool.into_iter().take(OVERVIEW_TOOLS).collect(),
            agents_asked: u32::try_from(agents).unwrap_or(u32::MAX),
        })
    }

    /// The files the thread changed and their lines, from the diffs the ledger kept.
    async fn change_sum(&self, thread_id: &str) -> Result<ChangeSum> {
        let rows: Vec<(String, Option<String>)> = sqlx::query_as(
            "SELECT path, diff FROM changes WHERE thread_id = ?
             ORDER BY created_at DESC, rowid DESC",
        )
        .bind(thread_id)
        .fetch_all(&self.pool)
        .await
        .context("read the thread's changes")?;
        // Paths in the order they were last changed, newest first.
        let mut paths: Vec<ChangedPath> = Vec::new();
        for (path, diff) in rows {
            let (added, removed) = diff.as_deref().map_or((0, 0), lines);
            match paths.iter_mut().find(|known| known.path == path) {
                Some(known) => {
                    known.added += added;
                    known.removed += removed;
                }
                None => paths.push(ChangedPath {
                    path,
                    added,
                    removed,
                }),
            }
        }
        Ok(ChangeSum {
            files: u32::try_from(paths.len()).unwrap_or(u32::MAX),
            added: paths.iter().map(|path| path.added).sum(),
            removed: paths.iter().map(|path| path.removed).sum(),
            newest: paths.into_iter().take(OVERVIEW_PATHS).collect(),
        })
    }

    /// The thread's log older than `before`, newest first: when each turn started and
    /// ended, and when its team started and ended.
    pub async fn events(
        &self,
        thread_id: &str,
        before: Option<i64>,
        limit: u32,
    ) -> Result<Vec<LogEntry>> {
        let rows: Vec<(i64, String, Option<i64>, Option<i64>)> = sqlx::query_as(
            "WITH numbered AS (
                SELECT ROW_NUMBER() OVER (ORDER BY rowid) AS number, status, started_at,
                    finished_at FROM turns WHERE thread_id = ?1
             ), log AS (
                SELECT started_at AS at, 'started' AS kind, number, NULL AS took FROM numbered
                UNION ALL
                SELECT finished_at, status, number, finished_at - started_at FROM numbered
                    WHERE finished_at IS NOT NULL
                UNION ALL
                SELECT started_at, 'team_started', NULL, NULL FROM teams WHERE thread_id = ?1
                UNION ALL
                SELECT ended_at, 'team_' || ended_why, NULL, ended_at - started_at FROM teams
                    WHERE thread_id = ?1 AND ended_at IS NOT NULL
             )
             SELECT at, kind, number, took FROM log WHERE ?2 IS NULL OR at < ?2
             ORDER BY at DESC, number DESC LIMIT ?3",
        )
        .bind(thread_id)
        .bind(before)
        .bind(limit.clamp(1, EVENTS_PAGE))
        .fetch_all(&self.pool)
        .await
        .context("read the thread's log")?;
        rows.into_iter()
            .map(|(at, kind, number, took)| log_entry(at, &kind, number, took))
            .collect()
    }
}

/// Lines added and removed in a unified diff without file headers.
fn lines(diff: &str) -> (u64, u64) {
    diff.lines().fold((0, 0), |(added, removed), line| {
        match line.as_bytes().first() {
            Some(b'+') => (added + 1, removed),
            Some(b'-') => (added, removed + 1),
            _ => (added, removed),
        }
    })
}

fn log_entry(at: i64, kind: &str, number: Option<i64>, took: Option<i64>) -> Result<LogEntry> {
    let turn = format!("Turn {}", number.unwrap_or_default());
    let took = took.map(duration_words).unwrap_or_default();
    let (kind, text) = match kind {
        "started" => (LogKind::Started, format!("{turn} started")),
        "completed" => (LogKind::Done, format!("{turn} done in {took}")),
        "interrupted" => (LogKind::Stopped, format!("{turn} stopped after {took}")),
        "failed" => (LogKind::Failed, format!("{turn} failed after {took}")),
        "team_started" => (LogKind::Started, "Team started".to_owned()),
        "team_done" => (LogKind::Done, format!("Team done in {took}")),
        other => {
            // A team that ended any other way stalled, went quiet, was stopped, or (under an
            // older core) ran out of a budget.
            let why = other.strip_prefix("team_").unwrap_or(other);
            (LogKind::Stopped, format!("Team ended ({why}) after {took}"))
        }
    };
    Ok(LogEntry {
        at,
        kind,
        text: dusk_protocol::thread::Line::new(&text)?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::TurnAnswer;
    use crate::{providers::ToolCall, testing::temp_state};

    fn call(id: &str, name: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: name.into(),
            arguments: serde_json::json!({}),
        }
    }

    /// The counts moved from the desktop's transcript to the core: what it counted from the
    /// tool calls of the thread's messages, the core now counts from `tool_executions`.
    #[tokio::test]
    async fn the_activity_counts_what_the_transcript_counted() {
        let state = temp_state("overview_activity").await;
        state.create_thread("thread", "test").await.unwrap();
        let steps = [
            vec![call("c1", "read_file"), call("c2", "run_command")],
            vec![call("c3", "read_file")],
            vec![call("c4", "edit_file"), call("c5", "read_file")],
        ];
        for (index, calls) in steps.into_iter().enumerate() {
            let uid = format!("u{index}");
            state.add_user_message("thread", &uid, "go").await.unwrap();
            let turn = state.start_turn("thread").await.unwrap().unwrap();
            let answer = TurnAnswer {
                content: String::new(),
                reasoning: None,
            };
            state
                .record_tool_calls(&turn.turn_id, answer, calls)
                .await
                .unwrap();
            state.interrupt_running_turns("thread").await.unwrap();
        }

        // The old `Transcript::activity`: each tool call of the messages, by name, most
        // used first and then by name.
        let mut counted: BTreeMap<String, u64> = BTreeMap::new();
        for message in state.list_messages("thread").await.unwrap() {
            for call in &message.tool_calls {
                *counted.entry(call.name.clone()).or_default() += 1;
            }
        }
        let mut counted: Vec<(String, u64)> = counted.into_iter().collect();
        counted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let activity = state.activity("thread").await.unwrap();
        let by_tool: Vec<(String, u64)> = activity
            .by_tool
            .into_iter()
            .map(|tool| (tool.name, tool.runs))
            .collect();
        assert_eq!(by_tool, counted);
        assert_eq!(activity.tool_runs, 5);
        assert_eq!(activity.agents_asked, 0);

        let log = state.events("thread", None, 100).await.unwrap();
        assert_eq!(log.len(), 6, "each of the three turns started and stopped");
    }
}
