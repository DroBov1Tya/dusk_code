//! Facts as agents and the user write them: a file's summary, its nodes and the edges
//! from them. Every write is checked here whatever the caller checked: kinds are closed
//! sets, texts have caps, a file has a bounded number of facts and a project too. A fact
//! is written against the file's hash at that moment, which is what makes it stale later.

use anyhow::{Context, Result, bail, ensure};
use sqlx::{Row, SqliteConnection};

use crate::state::{State, timestamp_ms};

/// Longest file summary, in characters.
pub const MAX_FILE_SUMMARY: usize = 400;
/// Longest node summary, in characters.
pub const MAX_NODE_SUMMARY: usize = 200;
/// Longest node name, in characters.
pub const MAX_NAME: usize = 128;
/// Longest edge target, in characters: a name, `path#name` or a path.
pub const MAX_TARGET: usize = 256;
/// Nodes and edges one file may hold.
pub const MAX_FILE_NODES: i64 = 200;
pub const MAX_FILE_EDGES: i64 = 400;
/// Nodes one project may hold.
pub const MAX_PROJECT_NODES: i64 = 200_000;

/// What a node is. A closed set, so a kind is never free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Module,
    Type,
    Function,
    Service,
    Test,
    Config,
    Doc,
    Other,
}

impl NodeKind {
    pub const ALL: [Self; 8] = [
        Self::Module,
        Self::Type,
        Self::Function,
        Self::Service,
        Self::Test,
        Self::Config,
        Self::Doc,
        Self::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Type => "type",
            Self::Function => "function",
            Self::Service => "service",
            Self::Test => "test",
            Self::Config => "config",
            Self::Doc => "doc",
            Self::Other => "other",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// How the source of an edge relates to its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    Uses,
    Imports,
    Calls,
    /// A test covers its target.
    Covers,
    Configures,
    Implements,
}

impl EdgeKind {
    pub const ALL: [Self; 6] = [
        Self::Uses,
        Self::Imports,
        Self::Calls,
        Self::Covers,
        Self::Configures,
        Self::Implements,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Uses => "uses",
            Self::Imports => "imports",
            Self::Calls => "calls",
            Self::Covers => "covers",
            Self::Configures => "configures",
            Self::Implements => "implements",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == text)
    }
}

/// Who wrote a fact: the thread, and whether the user said it.
#[derive(Debug, Clone, Copy)]
pub struct Author<'a> {
    /// `None` when no thread is known.
    pub thread: Option<&'a str>,
    /// Set by `fix`, which writes what the user said is right.
    pub by_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewNode {
    pub name: String,
    pub kind: NodeKind,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewEdge {
    /// A node of the same file by name; the file itself for `None`.
    pub from: Option<String>,
    pub to: String,
    pub kind: EdgeKind,
}

/// What `record` writes about one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Record {
    pub summary: Option<String>,
    pub nodes: Vec<NewNode>,
    pub edges: Vec<NewEdge>,
}

/// What a `record` did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Recorded {
    pub nodes: usize,
    pub edges: usize,
    /// Stale facts by agents the new ones replace.
    pub replaced: u64,
    /// Facts the user corrected that are still fresh, left as they are.
    pub kept: usize,
}

/// One fact `fix` changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixTarget {
    Summary,
    Node {
        name: String,
        kind: Option<NodeKind>,
    },
    Edge {
        from: Option<String>,
        to: String,
        kind: EdgeKind,
    },
}

impl State {
    /// Writes `record` about `path`, whose current hash is `hash`: the caller has just
    /// stored the file as it is (`graph_update`). Stale agent facts of the file that the
    /// record does not repeat are replaced by it; fresh facts the user corrected stay.
    pub async fn graph_record(
        &self,
        project: &str,
        path: &str,
        hash: &str,
        record: &Record,
        author: Author<'_>,
    ) -> Result<Recorded> {
        check_record(record)?;
        ensure!(
            hash.len() == 64,
            "the core does not keep facts about `{path}`"
        );
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin the record")?;
        let file = file_id(&mut transaction, project, path, hash).await?;
        let mut done = Recorded::default();
        if let Some(summary) = &record.summary {
            let user: Option<(bool, Option<String>)> =
                sqlx::query_as("SELECT by_user, summary_hash FROM graph_files WHERE id = ?")
                    .bind(file)
                    .fetch_optional(&mut *transaction)
                    .await
                    .context("read the summary")?;
            if !author.by_user
                && user.is_some_and(|(by_user, at)| by_user && at.as_deref() == Some(hash))
            {
                done.kept += 1;
            } else {
                sqlx::query(
                    "UPDATE graph_files SET summary = ?, summary_hash = ?, written_by = ?,
                         by_user = ?, updated_at = ? WHERE id = ?",
                )
                .bind(summary)
                .bind(hash)
                .bind(author.thread)
                .bind(author.by_user)
                .bind(now)
                .bind(file)
                .execute(&mut *transaction)
                .await
                .context("store the summary")?;
            }
        }
        for node in &record.nodes {
            let kept: Option<bool> = sqlx::query_scalar(
                "SELECT by_user = 1 AND source_hash = ? FROM graph_nodes
                 WHERE file_id = ? AND name = ? AND kind = ?",
            )
            .bind(hash)
            .bind(file)
            .bind(&node.name)
            .bind(node.kind.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .context("read a node")?;
            if !author.by_user && kept == Some(true) {
                done.kept += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO graph_nodes (file_id, name, kind, line, end_line, summary,
                     source_hash, written_by, by_user, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (file_id, name, kind) DO UPDATE SET line = excluded.line,
                     end_line = excluded.end_line, summary = excluded.summary,
                     source_hash = excluded.source_hash, written_by = excluded.written_by,
                     by_user = excluded.by_user, updated_at = excluded.updated_at",
            )
            .bind(file)
            .bind(&node.name)
            .bind(node.kind.as_str())
            .bind(node.line)
            .bind(node.end_line)
            .bind(&node.summary)
            .bind(hash)
            .bind(author.thread)
            .bind(author.by_user)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .context("store a node")?;
            done.nodes += 1;
        }
        if !record.nodes.is_empty() {
            done.replaced += sqlx::query(
                "DELETE FROM graph_nodes WHERE file_id = ? AND source_hash != ? AND by_user = 0",
            )
            .bind(file)
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .context("replace stale nodes")?
            .rows_affected();
        }
        for edge in &record.edges {
            let from = from_node(&mut transaction, file, edge.from.as_deref()).await?;
            let kept: Option<bool> = sqlx::query_scalar(
                "SELECT by_user = 1 AND source_hash = ? FROM graph_edges
                 WHERE from_file = ? AND from_node = ? AND to_name = ? AND kind = ?",
            )
            .bind(hash)
            .bind(file)
            .bind(from)
            .bind(&edge.to)
            .bind(edge.kind.as_str())
            .fetch_optional(&mut *transaction)
            .await
            .context("read an edge")?;
            if !author.by_user && kept == Some(true) {
                done.kept += 1;
                continue;
            }
            sqlx::query(
                "INSERT INTO graph_edges (from_file, from_node, to_name, kind, source_hash,
                     written_by, by_user, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (from_file, from_node, to_name, kind) DO UPDATE SET
                     source_hash = excluded.source_hash, written_by = excluded.written_by,
                     by_user = excluded.by_user, updated_at = excluded.updated_at",
            )
            .bind(file)
            .bind(from)
            .bind(&edge.to)
            .bind(edge.kind.as_str())
            .bind(hash)
            .bind(author.thread)
            .bind(author.by_user)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .context("store an edge")?;
            done.edges += 1;
        }
        if !record.edges.is_empty() {
            done.replaced += sqlx::query(
                "DELETE FROM graph_edges WHERE from_file = ? AND source_hash != ? AND by_user = 0",
            )
            .bind(file)
            .bind(hash)
            .execute(&mut *transaction)
            .await
            .context("replace stale edges")?
            .rows_affected();
        }
        check_counts(&mut transaction, project, file).await?;
        transaction.commit().await.context("store the record")?;
        Ok(done)
    }

    /// Changes one fact of `path` to `text`, or deletes it for `None`; false when there
    /// is no such fact. A changed fact is written against the file's current `hash`: who
    /// fixes it says it is right now. An edge has no text, so it can only be deleted.
    pub async fn graph_fix(
        &self,
        project: &str,
        path: &str,
        hash: &str,
        target: &FixTarget,
        text: Option<&str>,
        author: Author<'_>,
    ) -> Result<bool> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin the fix")?;
        let file: Option<i64> =
            sqlx::query_scalar("SELECT id FROM graph_files WHERE project = ? AND path = ?")
                .bind(project)
                .bind(path)
                .fetch_optional(&mut *transaction)
                .await
                .context("find the file")?;
        let Some(file) = file else {
            return Ok(false);
        };
        let stamp = (hash.len() == 64).then_some(hash);
        let changed = match (target, text) {
            (FixTarget::Summary, text) => {
                let text = text.unwrap_or_default();
                check_text("summary", text, MAX_FILE_SUMMARY)?;
                let at = if text.is_empty() { None } else { stamp };
                sqlx::query(
                    "UPDATE graph_files SET summary = ?, summary_hash = ?, written_by = ?,
                         by_user = ?, updated_at = ? WHERE id = ?",
                )
                .bind(text)
                .bind(at)
                .bind(author.thread)
                .bind(author.by_user)
                .bind(now)
                .bind(file)
                .execute(&mut *transaction)
                .await
                .context("fix the summary")?
                .rows_affected()
            }
            (FixTarget::Node { name, kind }, text) => {
                let ids: Vec<i64> = sqlx::query_scalar(
                    "SELECT id FROM graph_nodes WHERE file_id = ?1 AND name = ?2
                         AND (?3 IS NULL OR kind = ?3)",
                )
                .bind(file)
                .bind(name)
                .bind(kind.map(NodeKind::as_str))
                .fetch_all(&mut *transaction)
                .await
                .context("find the node")?;
                let id = match ids.as_slice() {
                    [] => return Ok(false),
                    [id] => *id,
                    _ => bail!("`{path}` has `{name}` of more than one kind; give `kind`"),
                };
                match text {
                    None => sqlx::query("DELETE FROM graph_nodes WHERE id = ?")
                        .bind(id)
                        .execute(&mut *transaction)
                        .await
                        .context("delete the node")?
                        .rows_affected(),
                    Some(text) => {
                        check_text("summary", text, MAX_NODE_SUMMARY)?;
                        let stamp =
                            stamp.context("the core does not keep facts about this file")?;
                        sqlx::query(
                            "UPDATE graph_nodes SET summary = ?, source_hash = ?, written_by = ?,
                                 by_user = ?, updated_at = ? WHERE id = ?",
                        )
                        .bind(text)
                        .bind(stamp)
                        .bind(author.thread)
                        .bind(author.by_user)
                        .bind(now)
                        .bind(id)
                        .execute(&mut *transaction)
                        .await
                        .context("fix the node")?
                        .rows_affected()
                    }
                }
            }
            (FixTarget::Edge { from, to, kind }, None) => {
                let from = match from {
                    Some(name) => match node_named(&mut transaction, file, name).await? {
                        Some(id) => id,
                        None => return Ok(false),
                    },
                    None => 0,
                };
                sqlx::query(
                    "DELETE FROM graph_edges
                     WHERE from_file = ? AND from_node = ? AND to_name = ? AND kind = ?",
                )
                .bind(file)
                .bind(from)
                .bind(to)
                .bind(kind.as_str())
                .execute(&mut *transaction)
                .await
                .context("delete the edge")?
                .rows_affected()
            }
            (FixTarget::Edge { .. }, Some(_)) => {
                bail!("an edge has no text: delete it and `record` the right one")
            }
        };
        transaction.commit().await.context("store the fix")?;
        Ok(changed > 0)
    }
}

/// The id of the stored file `path` when its hash is `hash`.
async fn file_id(
    connection: &mut SqliteConnection,
    project: &str,
    path: &str,
    hash: &str,
) -> Result<i64> {
    let row = sqlx::query("SELECT id, hash FROM graph_files WHERE project = ? AND path = ?")
        .bind(project)
        .bind(path)
        .fetch_optional(connection)
        .await
        .context("find the file")?
        .with_context(|| format!("`{path}` is not in the project graph"))?;
    let stored: String = row.try_get("hash")?;
    ensure!(
        stored == hash,
        "`{path}` changed while it was recorded; record it again"
    );
    Ok(row.try_get("id")?)
}

/// The node an edge starts from: 0 for the file, else the node of that name.
async fn from_node(
    connection: &mut SqliteConnection,
    file: i64,
    from: Option<&str>,
) -> Result<i64> {
    let Some(name) = from else {
        return Ok(0);
    };
    node_named(connection, file, name).await?.with_context(|| {
        format!("the edge starts at `{name}`, which this file has no node of; add it to `nodes`")
    })
}

async fn node_named(
    connection: &mut SqliteConnection,
    file: i64,
    name: &str,
) -> Result<Option<i64>> {
    sqlx::query_scalar(
        "SELECT id FROM graph_nodes WHERE file_id = ? AND name = ? ORDER BY id LIMIT 1",
    )
    .bind(file)
    .bind(name)
    .fetch_optional(connection)
    .await
    .context("find the node")
}

async fn check_counts(connection: &mut SqliteConnection, project: &str, file: i64) -> Result<()> {
    let (nodes, edges): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM graph_nodes WHERE file_id = ?1),
                (SELECT COUNT(*) FROM graph_edges WHERE from_file = ?1)",
    )
    .bind(file)
    .fetch_one(&mut *connection)
    .await
    .context("count the file's facts")?;
    ensure!(
        nodes <= MAX_FILE_NODES,
        "a file holds at most {MAX_FILE_NODES} nodes"
    );
    ensure!(
        edges <= MAX_FILE_EDGES,
        "a file holds at most {MAX_FILE_EDGES} edges"
    );
    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM graph_nodes n JOIN graph_files f ON f.id = n.file_id
         WHERE f.project = ?",
    )
    .bind(project)
    .fetch_one(connection)
    .await
    .context("count the project's nodes")?;
    ensure!(
        total <= MAX_PROJECT_NODES,
        "the project graph holds at most {MAX_PROJECT_NODES} nodes"
    );
    Ok(())
}

/// The checks of the tables and a few more, before anything is written.
fn check_record(record: &Record) -> Result<()> {
    if let Some(summary) = &record.summary {
        check_text("summary", summary, MAX_FILE_SUMMARY)?;
    }
    for node in &record.nodes {
        check_name("a node name", &node.name, MAX_NAME)?;
        check_text("a node summary", &node.summary, MAX_NODE_SUMMARY)?;
        match (node.line, node.end_line) {
            (Some(0), _) | (_, Some(0)) => bail!("lines count from 1"),
            (None, Some(_)) => bail!("`end_line` needs `line`"),
            (Some(line), Some(end)) if end < line => bail!("`end_line` is before `line`"),
            _ => {}
        }
    }
    for edge in &record.edges {
        if let Some(from) = &edge.from {
            check_name("an edge's `from`", from, MAX_NAME)?;
        }
        check_name("an edge's `to`", &edge.to, MAX_TARGET)?;
    }
    Ok(())
}

fn check_name(what: &str, text: &str, cap: usize) -> Result<()> {
    ensure!(!text.trim().is_empty(), "{what} is empty");
    check_text(what, text, cap)
}

fn check_text(what: &str, text: &str, cap: usize) -> Result<()> {
    ensure!(
        text.chars().count() <= cap,
        "{what} is longer than {cap} characters"
    );
    ensure!(
        !text.chars().any(char::is_control),
        "{what} has a control character"
    );
    Ok(())
}
