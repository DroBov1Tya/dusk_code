//! The variables of a project: a name and a secret value each, set by the user and
//! given to the commands that run in the project's folder.

use anyhow::{Context, Result};
use sqlx::Row;

use crate::state::State;

impl State {
    /// The names a project has, sorted; the values stay in the database.
    pub async fn project_env_names(&self, project_id: &str) -> Result<Vec<String>> {
        sqlx::query_scalar("SELECT name FROM project_env WHERE project_id = ? ORDER BY name")
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .context("list the project variables")
    }

    pub async fn set_project_env(&self, project_id: &str, name: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO project_env (project_id, name, value) VALUES (?, ?, ?)
             ON CONFLICT (project_id, name) DO UPDATE SET value = excluded.value",
        )
        .bind(project_id)
        .bind(name)
        .bind(value)
        .execute(&self.pool)
        .await
        .context("save the project variable; does the project exist?")?;
        Ok(())
    }

    pub async fn remove_project_env(&self, project_id: &str, name: &str) -> Result<()> {
        sqlx::query("DELETE FROM project_env WHERE project_id = ? AND name = ?")
            .bind(project_id)
            .bind(name)
            .execute(&self.pool)
            .await
            .context("remove the project variable")?;
        Ok(())
    }

    /// Names and values for a command that runs in `dir`, the folder of a saved project;
    /// none for any other folder.
    pub async fn project_env_for_dir(&self, dir: &str) -> Result<Vec<(String, String)>> {
        let rows = sqlx::query(
            "SELECT e.name, e.value FROM project_env e
             JOIN projects p ON p.id = e.project_id WHERE p.dir = ? ORDER BY e.name",
        )
        .bind(dir)
        .fetch_all(&self.pool)
        .await
        .context("read the project variables")?;
        rows.iter()
            .map(|row| Ok((row.try_get("name")?, row.try_get("value")?)))
            .collect::<sqlx::Result<_>>()
            .context("read the project variables")
    }
}

#[cfg(test)]
mod tests {
    use crate::testing::temp_state;

    #[tokio::test]
    async fn variables_belong_to_their_project_and_go_with_it() {
        let state = temp_state("project_env").await;
        state.create_project("a", "A", "", "/work/a").await.unwrap();
        state.create_project("b", "B", "", "/work/b").await.unwrap();
        state.set_project_env("a", "TOKEN", "one").await.unwrap();
        state.set_project_env("a", "TOKEN", "two").await.unwrap();
        state.set_project_env("b", "OTHER", "x").await.unwrap();

        assert_eq!(state.project_env_names("a").await.unwrap(), ["TOKEN"]);
        assert_eq!(
            state.project_env_for_dir("/work/a").await.unwrap(),
            [("TOKEN".to_owned(), "two".to_owned())]
        );
        assert!(state.project_env_for_dir("/work").await.unwrap().is_empty());
        assert!(state.set_project_env("missing", "X", "y").await.is_err());

        state.remove_project("a").await.unwrap();
        assert!(
            state
                .project_env_for_dir("/work/a")
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.project_env_names("b").await.unwrap(), ["OTHER"]);
    }
}
