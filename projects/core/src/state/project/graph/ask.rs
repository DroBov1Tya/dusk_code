//! Questions to the graph: one file with its facts, nodes by name, edges into a file or
//! a node, a search, and what depends on a set of files. Every answer is capped by the
//! caller's `limit`; one row more than shown is read to say there are more.

use std::collections::{BTreeMap, HashSet};

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use super::like_escaped;
use crate::state::State;

/// A file with its summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileView {
    pub path: String,
    pub lines: u64,
    pub bytes: u64,
    /// Whether the core hashed it: facts are kept only about such a file.
    pub hashed: bool,
    pub summary: String,
    pub stale: bool,
    pub by_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeView {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub line: Option<u32>,
    pub end_line: Option<u32>,
    pub summary: String,
    pub stale: bool,
    pub by_user: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeView {
    /// The file the edge starts in.
    pub path: String,
    /// The node it starts at; the file itself for `None`.
    pub from: Option<String>,
    pub to: String,
    pub kind: String,
    pub stale: bool,
}

/// A row `find` found: a file by its path or summary, or a node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    File(FileView),
    Node(NodeView),
}

/// A file that may be affected by a change: how far and through what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Impacted {
    pub path: String,
    /// 1 when an edge of it points into the changed files, 2 through one such file.
    pub hops: u8,
    /// The edge kinds that lead to it, sorted.
    pub kinds: Vec<String>,
    /// The file it depends on, of those one hop nearer.
    pub via: String,
}

macro_rules! file_columns {
    () => {
        "f.path, f.lines, f.bytes, f.hash != '' AS hashed, f.summary,
    f.summary_hash IS NOT NULL AND f.summary_hash != f.hash AS stale, f.by_user"
    };
}
macro_rules! node_columns {
    () => {
        "f.path, n.name, n.kind, n.line, n.end_line, n.summary,
    n.source_hash != f.hash AS stale, n.by_user"
    };
}
macro_rules! edge_columns {
    () => {
        "f.path, (SELECT name FROM graph_nodes WHERE id = e.from_node)
    AS from_name, e.to_name, e.kind, e.source_hash != f.hash AS stale"
    };
}
/// Edges whose target is the file `?2` (relative path) or one of its nodes, in the
/// project `?1`, from another file.
const INTO_FILE: &str = "SELECT f.path, (SELECT name FROM graph_nodes WHERE id = e.from_node)
        AS from_name, e.to_name, e.kind, e.source_hash != f.hash AS stale
    FROM graph_edges e JOIN graph_files f ON f.id = e.from_file
    WHERE f.project = ?1 AND f.path != ?2 AND (e.to_name = ?2
        OR substr(e.to_name, 1, length(?2) + 1) = ?2 || '#'
        OR e.to_name IN (SELECT n.name FROM graph_nodes n JOIN graph_files t ON t.id = n.file_id
            WHERE t.project = ?1 AND t.path = ?2))
    ORDER BY f.path, e.kind, e.to_name LIMIT ?3";

impl State {
    /// The stored file `path`.
    pub async fn graph_file(&self, project: &str, path: &str) -> Result<Option<FileView>> {
        sqlx::query(concat!(
            "SELECT ",
            file_columns!(),
            " FROM graph_files f WHERE f.project = ? AND f.path = ?"
        ))
        .bind(project)
        .bind(path)
        .try_map(file_view)
        .fetch_optional(&self.pool)
        .await
        .context("read a file of the graph")
    }

    /// The nodes of the file `path`, in line order.
    pub async fn graph_file_nodes(
        &self,
        project: &str,
        path: &str,
        limit: i64,
    ) -> Result<Vec<NodeView>> {
        sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM graph_nodes n JOIN graph_files f ON f.id = n.file_id
             WHERE f.project = ? AND f.path = ?
             ORDER BY n.line IS NULL, n.line, n.name LIMIT ?"
        ))
        .bind(project)
        .bind(path)
        .bind(limit)
        .try_map(node_view)
        .fetch_all(&self.pool)
        .await
        .context("read the file's nodes")
    }

    /// The edges from the file `path` and its nodes.
    pub async fn graph_edges_from(
        &self,
        project: &str,
        path: &str,
        node: Option<&str>,
        limit: i64,
    ) -> Result<Vec<EdgeView>> {
        sqlx::query(concat!(
            "SELECT ",
            edge_columns!(),
            " FROM graph_edges e JOIN graph_files f ON f.id = e.from_file
             WHERE f.project = ?1 AND f.path = ?2
                 AND (?3 IS NULL OR e.from_node IN
                     (SELECT id FROM graph_nodes WHERE file_id = f.id AND name = ?3))
             ORDER BY e.from_node, e.kind, e.to_name LIMIT ?4"
        ))
        .bind(project)
        .bind(path)
        .bind(node)
        .bind(limit)
        .try_map(edge_view)
        .fetch_all(&self.pool)
        .await
        .context("read the edges from the file")
    }

    /// The edges of other files into the file `path` or its nodes.
    pub async fn graph_edges_into_file(
        &self,
        project: &str,
        path: &str,
        limit: i64,
    ) -> Result<Vec<EdgeView>> {
        sqlx::query(INTO_FILE)
            .bind(project)
            .bind(path)
            .bind(limit)
            .try_map(edge_view)
            .fetch_all(&self.pool)
            .await
            .context("read the edges into the file")
    }

    /// The nodes named `name`, in any file.
    pub async fn graph_nodes_named(
        &self,
        project: &str,
        name: &str,
        limit: i64,
    ) -> Result<Vec<NodeView>> {
        sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM graph_nodes n JOIN graph_files f ON f.id = n.file_id
             WHERE f.project = ? AND n.name = ? ORDER BY f.path, n.kind LIMIT ?"
        ))
        .bind(project)
        .bind(name)
        .bind(limit)
        .try_map(node_view)
        .fetch_all(&self.pool)
        .await
        .context("read the nodes")
    }

    /// The edges into the node `name` of `path`: by its name or by `path#name`.
    pub async fn graph_edges_into_node(
        &self,
        project: &str,
        path: &str,
        name: &str,
        limit: i64,
    ) -> Result<Vec<EdgeView>> {
        sqlx::query(concat!(
            "SELECT ",
            edge_columns!(),
            " FROM graph_edges e JOIN graph_files f ON f.id = e.from_file
             WHERE f.project = ?1 AND (e.to_name = ?2 OR e.to_name = ?3 || '#' || ?2)
             ORDER BY f.path, e.kind LIMIT ?4"
        ))
        .bind(project)
        .bind(name)
        .bind(path)
        .bind(limit)
        .try_map(edge_view)
        .fetch_all(&self.pool)
        .await
        .context("read the edges into the node")
    }

    /// Files whose path or summary holds `text`, then nodes whose name, kind or summary
    /// does, in any case; at most `limit` of each.
    pub async fn graph_find(&self, project: &str, text: &str, limit: i64) -> Result<Vec<Found>> {
        let pattern = format!("%{}%", like_escaped(&text.to_lowercase()));
        let files = sqlx::query(concat!(
            "SELECT ",
            file_columns!(),
            " FROM graph_files f
             WHERE f.project = ?1 AND (lower(f.path) LIKE ?2 ESCAPE '\\'
                 OR lower(f.summary) LIKE ?2 ESCAPE '\\')
             ORDER BY f.summary = '', f.path LIMIT ?3"
        ))
        .bind(project)
        .bind(&pattern)
        .bind(limit)
        .try_map(file_view)
        .fetch_all(&self.pool)
        .await
        .context("search the graph's files")?;
        let nodes = sqlx::query(concat!(
            "SELECT ",
            node_columns!(),
            " FROM graph_nodes n JOIN graph_files f ON f.id = n.file_id
             WHERE f.project = ?1 AND (lower(n.name) LIKE ?2 ESCAPE '\\' OR n.kind = ?3
                 OR lower(n.summary) LIKE ?2 ESCAPE '\\')
             ORDER BY lower(n.name) != ?3, f.path, n.name LIMIT ?4"
        ))
        .bind(project)
        .bind(&pattern)
        .bind(text.to_lowercase())
        .bind(limit)
        .try_map(node_view)
        .fetch_all(&self.pool)
        .await
        .context("search the graph's nodes")?;
        Ok(files
            .into_iter()
            .map(Found::File)
            .chain(nodes.into_iter().map(Found::Node))
            .collect())
    }

    /// The files within two hops of `paths` against the edges: those with an edge into
    /// one of them, then those with an edge into those. At most `limit` files; `paths`
    /// themselves are left out.
    pub async fn graph_impact(
        &self,
        project: &str,
        paths: &[String],
        limit: usize,
    ) -> Result<Vec<Impacted>> {
        let mut seen: HashSet<String> = paths.iter().cloned().collect();
        let mut found: Vec<Impacted> = Vec::new();
        let mut frontier: Vec<String> = paths.to_vec();
        for hops in 1..=2u8 {
            let mut next = Vec::new();
            for target in &frontier {
                let row_cap = i64::try_from(limit * 4).unwrap_or(i64::MAX);
                let edges = self.graph_edges_into_file(project, target, row_cap).await?;
                let mut by_file: BTreeMap<String, Vec<String>> = BTreeMap::new();
                for edge in edges {
                    by_file.entry(edge.path).or_default().push(edge.kind);
                }
                for (path, mut kinds) in by_file {
                    if found.len() >= limit {
                        return Ok(found);
                    }
                    if !seen.insert(path.clone()) {
                        continue;
                    }
                    kinds.sort();
                    kinds.dedup();
                    next.push(path.clone());
                    found.push(Impacted {
                        path,
                        hops,
                        kinds,
                        via: target.clone(),
                    });
                }
            }
            frontier = next;
        }
        Ok(found)
    }
}

fn file_view(row: SqliteRow) -> sqlx::Result<FileView> {
    let lines: i64 = row.try_get("lines")?;
    let bytes: i64 = row.try_get("bytes")?;
    Ok(FileView {
        path: row.try_get("path")?,
        lines: u64::try_from(lines).unwrap_or(0),
        bytes: u64::try_from(bytes).unwrap_or(0),
        hashed: row.try_get("hashed")?,
        summary: row.try_get("summary")?,
        stale: row.try_get("stale")?,
        by_user: row.try_get("by_user")?,
    })
}

fn node_view(row: SqliteRow) -> sqlx::Result<NodeView> {
    Ok(NodeView {
        path: row.try_get("path")?,
        name: row.try_get("name")?,
        kind: row.try_get("kind")?,
        line: row.try_get("line")?,
        end_line: row.try_get("end_line")?,
        summary: row.try_get("summary")?,
        stale: row.try_get("stale")?,
        by_user: row.try_get("by_user")?,
    })
}

fn edge_view(row: SqliteRow) -> sqlx::Result<EdgeView> {
    Ok(EdgeView {
        path: row.try_get("path")?,
        from: row.try_get("from_name")?,
        to: row.try_get("to_name")?,
        kind: row.try_get("kind")?,
        stale: row.try_get("stale")?,
    })
}
