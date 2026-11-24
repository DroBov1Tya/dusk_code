//! The project graph's store. For each saved project: every file the core walked, with
//! numbers it computes itself (lines, bytes, hash, change time), and the facts agents
//! or the user wrote about a file: a summary, the nodes it holds, the edges from them.
//! A fact is stale when the file's hash is no longer the one it was written against;
//! stale facts are kept and marked, and only a file that is gone takes its facts along.
//! `facts` writes facts, `ask` answers questions; the wording is the tool's.

mod ask;
mod facts;
#[cfg(test)]
mod tests;

use std::collections::HashMap;

use anyhow::{Context, Result};
use sqlx::Row;

use crate::state::{State, timestamp_ms};

pub use ask::{EdgeView, FileView, Found, NodeView};
pub use facts::{Author, EdgeKind, FixTarget, NewEdge, NewNode, NodeKind, Record};

/// Rows written in one transaction while a walk is stored.
const CHUNK: usize = 500;

/// A file as the core measured it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalkedFile {
    /// Relative to the project, `/` between parts.
    pub path: String,
    /// `\n` bytes; 0 for a file the core did not read.
    pub lines: u64,
    pub bytes: u64,
    /// SHA-256 in hex; empty for a file the core does not hash.
    pub hash: String,
    pub modified_ms: i64,
}

/// What a walk compares a file on disk against: equal stamps mean it is not read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphStamp {
    pub bytes: u64,
    pub modified_ms: i64,
}

/// A file as the map shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MapEntry {
    pub path: String,
    pub lines: u64,
    pub summary: String,
    /// The summary was written against another version of the file.
    pub stale: bool,
}

impl State {
    /// The stamps of the stored files of `project`, by path.
    pub async fn graph_stamps(&self, project: &str) -> Result<HashMap<String, GraphStamp>> {
        let rows =
            sqlx::query("SELECT path, bytes, modified_ms FROM graph_files WHERE project = ?")
                .bind(project)
                .fetch_all(&self.pool)
                .await
                .context("read the graph's files")?;
        rows.iter()
            .map(|row| {
                let bytes: i64 = row.try_get("bytes")?;
                Ok((
                    row.try_get("path")?,
                    GraphStamp {
                        bytes: u64::try_from(bytes).unwrap_or(0),
                        modified_ms: row.try_get("modified_ms")?,
                    },
                ))
            })
            .collect()
    }

    /// Stores what a walk measured and drops the files it no longer found, with their
    /// facts. A file's facts stay when its numbers change: they are stale from then on.
    pub async fn graph_update(
        &self,
        project: &str,
        files: &[WalkedFile],
        gone: &[String],
    ) -> Result<()> {
        let now = timestamp_ms()?;
        for chunk in files.chunks(CHUNK) {
            let mut transaction = self.pool.begin().await.context("begin the graph update")?;
            for file in chunk {
                sqlx::query(
                    "INSERT INTO graph_files (project, path, lines, bytes, hash, modified_ms,
                         updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?)
                     ON CONFLICT (project, path) DO UPDATE SET lines = excluded.lines,
                         bytes = excluded.bytes, hash = excluded.hash,
                         modified_ms = excluded.modified_ms",
                )
                .bind(project)
                .bind(&file.path)
                .bind(i64::try_from(file.lines).unwrap_or(i64::MAX))
                .bind(i64::try_from(file.bytes).unwrap_or(i64::MAX))
                .bind(&file.hash)
                .bind(file.modified_ms)
                .bind(now)
                .execute(&mut *transaction)
                .await
                .context("store a file of the graph")?;
            }
            transaction.commit().await.context("store the graph")?;
        }
        for chunk in gone.chunks(CHUNK) {
            let mut transaction = self.pool.begin().await.context("begin the graph update")?;
            for path in chunk {
                sqlx::query("DELETE FROM graph_files WHERE project = ? AND path = ?")
                    .bind(project)
                    .bind(path)
                    .execute(&mut *transaction)
                    .await
                    .context("drop a file of the graph")?;
            }
            transaction.commit().await.context("store the graph")?;
        }
        Ok(())
    }

    /// The files under `folder` (relative, no `/` at the end; the project for `None`),
    /// sorted by path.
    pub async fn graph_map(&self, project: &str, folder: Option<&str>) -> Result<Vec<MapEntry>> {
        let prefix = folder.map(|folder| format!("{}/%", like_escaped(folder)));
        let rows = sqlx::query(
            "SELECT path, lines, summary, summary_hash IS NOT NULL AND summary_hash != hash
                 AS stale
             FROM graph_files
             WHERE project = ?1 AND (?2 IS NULL OR path LIKE ?2 ESCAPE '\\')
             ORDER BY path",
        )
        .bind(project)
        .bind(prefix)
        .fetch_all(&self.pool)
        .await
        .context("read the graph's map")?;
        rows.iter()
            .map(|row| {
                let lines: i64 = row.try_get("lines")?;
                Ok(MapEntry {
                    path: row.try_get("path")?,
                    lines: u64::try_from(lines).unwrap_or(0),
                    summary: row.try_get("summary")?,
                    stale: row.try_get("stale")?,
                })
            })
            .collect()
    }
}

/// Forgets the graph of `project`, in the caller's transaction: its folder now holds
/// something else.
pub(super) async fn forget(connection: &mut sqlx::SqliteConnection, project: &str) -> Result<()> {
    sqlx::query("DELETE FROM graph_files WHERE project = ?")
        .bind(project)
        .execute(connection)
        .await
        .context("forget the old folder's graph")?;
    Ok(())
}

/// `text` with the wildcards of `LIKE` escaped by `\`.
fn like_escaped(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}
