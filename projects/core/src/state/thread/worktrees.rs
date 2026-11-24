//! The worktree a thread works in, when it branched off its folder (`tools::git`). The
//! record is what makes the thread's tools follow it there, across a restart too.

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadWorktree {
    pub thread_id: String,
    /// The worktree's folder, resolved.
    pub dir: String,
    /// Where `base` is checked out: the folder the branch was made from.
    pub worktree_of: String,
    /// The folder of the saved project the work belongs to.
    pub project: String,
    pub branch: String,
    pub base: String,
    /// The thread that made it: the thread itself or the one that asked it.
    pub made_by: String,
}

impl State {
    /// Fails when the thread already has one or the folder is taken.
    pub async fn add_thread_worktree(&self, worktree: &ThreadWorktree) -> Result<()> {
        sqlx::query(
            "INSERT INTO thread_worktrees
                (thread_id, dir, worktree_of, project, branch, base, made_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&worktree.thread_id)
        .bind(&worktree.dir)
        .bind(&worktree.worktree_of)
        .bind(&worktree.project)
        .bind(&worktree.branch)
        .bind(&worktree.base)
        .bind(&worktree.made_by)
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("store the thread's worktree")?;
        Ok(())
    }

    pub async fn thread_worktree(&self, thread_id: &str) -> Result<Option<ThreadWorktree>> {
        sqlx::query("SELECT * FROM thread_worktrees WHERE thread_id = ?")
            .bind(thread_id)
            .fetch_optional(&self.pool)
            .await
            .context("read the thread's worktree")?
            .map(decode)
            .transpose()
    }

    /// The worktrees `thread_id` made: its own and those of the agents it asked.
    pub async fn worktrees_made_by(&self, thread_id: &str) -> Result<Vec<ThreadWorktree>> {
        sqlx::query("SELECT * FROM thread_worktrees WHERE made_by = ? ORDER BY created_at, dir")
            .bind(thread_id)
            .fetch_all(&self.pool)
            .await
            .context("read the thread's worktrees")?
            .into_iter()
            .map(decode)
            .collect()
    }

    /// The record of the worktree at `dir`, whoever works there.
    pub async fn worktree_at(&self, dir: &str) -> Result<Option<ThreadWorktree>> {
        sqlx::query("SELECT * FROM thread_worktrees WHERE dir = ?")
            .bind(dir)
            .fetch_optional(&self.pool)
            .await
            .context("read the worktree")?
            .map(decode)
            .transpose()
    }

    /// The thread works in its own folder again.
    pub async fn remove_thread_worktree(&self, thread_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM thread_worktrees WHERE thread_id = ?")
            .bind(thread_id)
            .execute(&self.pool)
            .await
            .context("forget the thread's worktree")?;
        Ok(())
    }
}

fn decode(row: SqliteRow) -> Result<ThreadWorktree> {
    Ok(ThreadWorktree {
        thread_id: row.try_get("thread_id")?,
        dir: row.try_get("dir")?,
        worktree_of: row.try_get("worktree_of")?,
        project: row.try_get("project")?,
        branch: row.try_get("branch")?,
        base: row.try_get("base")?,
        made_by: row.try_get("made_by")?,
    })
}

#[cfg(test)]
mod tests {
    use super::ThreadWorktree;
    use crate::testing::temp_dir;

    fn row(thread: &str, dir: &str) -> ThreadWorktree {
        ThreadWorktree {
            thread_id: thread.into(),
            dir: dir.into(),
            worktree_of: "/p".into(),
            project: "/p".into(),
            branch: format!("agent/{thread}"),
            base: "main".into(),
            made_by: "chat".into(),
        }
    }

    /// A thread has one worktree at most, a folder belongs to one thread, the record
    /// survives a restart, and a deleted chat takes its records along.
    #[tokio::test]
    async fn a_thread_keeps_one_worktree_across_a_restart() {
        let path = temp_dir("thread_worktrees").join("state.db");
        let state = crate::state::State::open(&path).await.unwrap();
        state.create_thread("chat", "test").await.unwrap();
        state.create_thread("other", "test").await.unwrap();
        state
            .add_thread_worktree(&row("chat", "/w/a"))
            .await
            .unwrap();
        assert!(
            state
                .add_thread_worktree(&row("chat", "/w/b"))
                .await
                .is_err()
        );
        assert!(
            state
                .add_thread_worktree(&row("other", "/w/a"))
                .await
                .is_err()
        );
        state.close().await;
        let state = crate::state::State::open(&path).await.unwrap();
        assert_eq!(
            state.thread_worktree("chat").await.unwrap(),
            Some(row("chat", "/w/a"))
        );
        assert_eq!(state.worktrees_made_by("chat").await.unwrap().len(), 1);
        assert!(state.worktree_at("/w/a").await.unwrap().is_some());
        state.delete_thread("chat").await.unwrap();
        assert_eq!(state.thread_worktree("chat").await.unwrap(), None);
    }
}
