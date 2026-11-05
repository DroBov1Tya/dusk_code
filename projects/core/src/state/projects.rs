//! Projects: named folders. Their chats are found by folder, so none is stored here.

use std::path::PathBuf;

use anyhow::{Context, Result};
use sqlx::{Row, sqlite::SqliteRow};

use super::{State, timestamp_ms};

pub use dusk_protocol::project::Project;

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

    pub async fn update_project(
        &self,
        id: &str,
        name: &str,
        description: &str,
        dir: &str,
    ) -> Result<bool> {
        let done = sqlx::query("UPDATE projects SET name = ?, description = ?, dir = ? WHERE id = ?")
            .bind(name)
            .bind(description)
            .bind(dir)
            .bind(id)
            .execute(&self.pool)
            .await
            .context("save the project; is its folder another project's?")?;
        Ok(done.rows_affected() > 0)
    }

    pub async fn remove_project(&self, id: &str) -> Result<()> {
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .context("remove the project")?;
        Ok(())
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
    use crate::testing::temp_state;

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
}
