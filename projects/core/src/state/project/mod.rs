//! Projects: named folders. Their chats are found by folder, so none is stored here.
//! `env` keeps a project's variables, `schedules` its scheduled tasks, `episodes` what
//! ended a problem in it, `graph` the index of its source.

mod env;
mod episodes;
pub mod graph;
mod schedules;

use std::path::PathBuf;

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

pub use dusk_protocol::project::Project;
pub use episodes::{Episode, EpisodeKind, NewEpisode, Recall};

impl State {
    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        sqlx::query("SELECT * FROM projects ORDER BY name COLLATE NOCASE")
            .try_map(project_from_row)
            .fetch_all(&self.pool)
            .await
            .context("list projects")
    }

    pub async fn load_project(&self, id: &str) -> Result<Option<Project>> {
        sqlx::query("SELECT * FROM projects WHERE id = ?")
            .bind(id)
            .try_map(project_from_row)
            .fetch_optional(&self.pool)
            .await
            .context("read the project")
    }

    pub async fn create_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
        dir: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO projects (id, name, description, dir, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(dir)
        .bind(timestamp_ms()?)
        .execute(&self.pool)
        .await
        .context("save the project; is its folder another project's?")?;
        Ok(())
    }

    /// Changes a project; false when there is none with `id`. A new folder starts with
    /// no episodes and no graph: both describe the old folder, and an episode written
    /// from one repository must not be recalled in another. The foreign keys would carry
    /// them over (`ON UPDATE CASCADE`), so they are deleted first, in the same
    /// transaction.
    pub async fn update_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
        dir: &str,
    ) -> Result<bool> {
        let mut transaction = self.pool.begin().await.context("begin the project save")?;
        let old: Option<String> = sqlx::query_scalar("SELECT dir FROM projects WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *transaction)
            .await
            .context("read the project")?;
        let Some(old) = old else {
            return Ok(false);
        };
        let moved = old != dir;
        if moved {
            for sql in [
                "DELETE FROM episodes WHERE project = ?",
                "DELETE FROM graph_files WHERE project = ?",
            ] {
                sqlx::query(sql)
                    .bind(&old)
                    .execute(&mut *transaction)
                    .await
                    .context("forget what the old folder held")?;
            }
        }
        sqlx::query("UPDATE projects SET name = ?, description = ?, dir = ? WHERE id = ?")
            .bind(name)
            .bind(description)
            .bind(dir)
            .bind(id)
            .execute(&mut *transaction)
            .await
            .context("save the project; is its folder another project's?")?;
        transaction.commit().await.context("save the project")?;
        if moved {
            self.load_episode_counts().await?;
        }
        Ok(true)
    }

    pub async fn remove_project(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("remove the project")?;
        // Its episodes went with it.
        self.load_episode_counts().await
    }
}

fn project_from_row(row: SqliteRow) -> sqlx::Result<Project> {
    Ok(Project {
        id: row.try_get("id")?,
        name: row.try_get("name")?,
        description: row.try_get("description")?,
        dir: PathBuf::from(row.try_get::<String, _>("dir")?),
        created_at: row.try_get("created_at")?,
    })
}

#[cfg(test)]
mod tests {
    use crate::{
        project::graph::{Language, SymbolKind},
        state::{
            EpisodeKind, NewEpisode, State,
            graph::{IndexedFile, NewSymbol, Stamp},
        },
        testing::temp_state,
    };

    #[tokio::test]
    async fn a_folder_is_one_project() {
        let state = temp_state("projects").await;
        state.create_project("a", "A", "", "/x").await.unwrap();
        assert!(state.create_project("b", "B", "", "/x").await.is_err());
        assert!(state.update_project("a", "A2", "d", "/y").await.unwrap());
        let projects = state.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "A2");
        state.remove_project("a").await.unwrap();
        assert!(state.load_project("a").await.unwrap().is_none());
    }

    async fn count(state: &State, sql: &'static str, project: &str) -> i64 {
        sqlx::query_scalar(sql)
            .bind(project)
            .fetch_one(&state.pool)
            .await
            .unwrap()
    }

    /// Episodes and the graph describe a folder: a project moved to another folder keeps
    /// neither, under the old name or the new one.
    #[tokio::test]
    async fn a_project_moved_to_another_folder_forgets_the_old_one() {
        let (a, b) = ("/work/repo-a", "/work/repo-b");
        let state = temp_state("project-moved").await;
        state.create_project("p", "P", "", a).await.unwrap();
        state
            .record_episode(&NewEpisode {
                project: a.into(),
                thread_id: None,
                turn_id: None,
                kind: EpisodeKind::Fixed,
                signature: format!("{:064x}", 1),
                tool: "run_command".into(),
                model: "m".into(),
                problem: "error: the linker of repo-a".into(),
                failed: String::new(),
                worked: "cargo build".into(),
                lesson: String::new(),
            })
            .await
            .unwrap();
        state.note_episode(a);
        let file = IndexedFile {
            path: "src/lib.rs".into(),
            language: Language::Rust,
            hash: "0".repeat(64),
            stamp: Stamp {
                size: 1,
                modified_ms: 1,
            },
            module: String::new(),
            is_test: false,
            skipped: None,
            symbols: vec![NewSymbol {
                name: "Alpha".into(),
                parent: None,
                kind: SymbolKind::Type,
                line: 1,
                end_line: 1,
            }],
            refs: Vec::new(),
        };
        state.replace_graph_files(a, &[file]).await.unwrap();
        assert!(state.has_episodes(a));

        // Only the name changes: what the folder held stays.
        assert!(state.update_project("p", "P2", "", a).await.unwrap());
        assert!(state.has_episodes(a));
        assert_eq!(state.graph_counts(a).await.unwrap().files, 1);

        assert!(state.update_project("p", "P2", "", b).await.unwrap());
        for project in [a, b] {
            let episodes = "SELECT COUNT(*) FROM episodes WHERE project = ?";
            let files = "SELECT COUNT(*) FROM graph_files WHERE project = ?";
            assert_eq!(count(&state, episodes, project).await, 0, "{project}");
            assert_eq!(count(&state, files, project).await, 0, "{project}");
            assert!(!state.has_episodes(project), "{project}");
        }
        for table in ["episodes_search", "graph_symbols"] {
            let sql = format!("SELECT COUNT(*) FROM {table}");
            let rows: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
                .fetch_one(&state.pool)
                .await
                .unwrap();
            assert_eq!(rows, 0, "{table}");
        }
        assert!(!state.update_project("missing", "M", "", b).await.unwrap());
    }

    #[tokio::test]
    async fn a_removed_project_or_episode_is_not_counted() {
        let state = temp_state("project-removed").await;
        state.create_project("p", "P", "", "/work/p").await.unwrap();
        let episode = NewEpisode {
            project: "/work/p".into(),
            thread_id: None,
            turn_id: None,
            kind: EpisodeKind::Fixed,
            signature: format!("{:064x}", 1),
            tool: "run_command".into(),
            model: "m".into(),
            problem: "error".into(),
            failed: String::new(),
            worked: "cargo build".into(),
            lesson: String::new(),
        };
        let id = state.record_episode(&episode).await.unwrap();
        state.note_episode("/work/p");
        assert!(state.remove_episode("/work/p", id).await.unwrap());
        assert!(!state.has_episodes("/work/p"));

        state.record_episode(&episode).await.unwrap();
        state.note_episode("/work/p");
        state.remove_project("p").await.unwrap();
        assert!(!state.has_episodes("/work/p"));
    }
}
