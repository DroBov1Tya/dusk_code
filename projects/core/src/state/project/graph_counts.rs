//! Counts the questions need beside their rows: how many definitions and users a name
//! has, so an answer can say how many it did not show and refuse a name too common to
//! answer, and the languages of a project's index, which a miss names.

use anyhow::{Context, Result};
use sqlx::Row;

use super::under;
use crate::{project::graph::Language, state::State};

/// How often a name occurs in a project, under a folder when one is given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NameCounts {
    /// Symbols of the name, inside `parent` when one is given.
    pub definitions: u64,
    /// Files that use or import the name.
    pub users: u64,
}

impl State {
    pub async fn graph_name_counts(
        &self,
        project: &str,
        name: &str,
        parent: Option<&str>,
        folder: Option<&str>,
    ) -> Result<NameCounts> {
        let row = sqlx::query(
            "SELECT
                (SELECT COUNT(*) FROM graph_symbols s JOIN graph_files f USING (file_id)
                 WHERE f.project = ?1 AND s.name = ?2 AND (?3 IS NULL OR s.parent = ?3)
                    AND (?4 IS NULL OR substr(f.path, 1, length(?4)) = ?4)) AS definitions,
                (SELECT COUNT(DISTINCT r.file_id) FROM graph_refs r JOIN graph_files f USING (file_id)
                 WHERE f.project = ?1 AND r.name = ?2 AND r.kind IN ('use', 'import')
                    AND (?4 IS NULL OR substr(f.path, 1, length(?4)) = ?4)) AS users",
        )
        .bind(project)
        .bind(name)
        .bind(parent)
        .bind(under(folder))
        .fetch_one(&self.pool)
        .await
        .context("count the name")?;
        let count = |column: &str| -> Result<u64> {
            u64::try_from(row.try_get::<i64, _>(column)?).context("negative count")
        };
        Ok(NameCounts {
            definitions: count("definitions")?,
            users: count("users")?,
        })
    }

    /// The languages of the project's indexed files, in the order of their names. A
    /// language this build does not read (written by a later version) is left out.
    pub async fn graph_languages(&self, project: &str) -> Result<Vec<Language>> {
        let names: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT language FROM graph_files WHERE project = ? ORDER BY language",
        )
        .bind(project)
        .fetch_all(&self.pool)
        .await
        .context("read the languages of the graph")?;
        Ok(names
            .iter()
            .filter_map(|name| Language::parse(name))
            .collect())
    }
}
