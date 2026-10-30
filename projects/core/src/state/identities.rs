//! Agent identities as last read from `agents/`: what each agent is, where its prompt
//! lives and which version of it is current.

use anyhow::{Context, Result};
use sqlx::Row;

use super::{State, timestamp_ms};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub name: String,
    /// The AGENT.md the prompt is read from.
    pub path: String,
    pub version: i64,
    pub content_hash: String,
    pub description: String,
    pub entry: bool,
    pub links: Vec<String>,
    /// `None` gives the agent every tool.
    pub tools: Option<Vec<String>>,
    pub updated_at: i64,
}

impl State {
    /// Makes the table match `found`: new agents start at version 1, a changed file
    /// raises the version, agents no longer found are dropped.
    pub async fn sync_identities(&self, found: &[Identity]) -> Result<Vec<Identity>> {
        let now = timestamp_ms()?;
        let mut transaction = self.pool.begin().await.context("begin identity sync")?;
        for identity in found {
            let tools = identity
                .tools
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            sqlx::query(
                "INSERT INTO identities (name, path, version, content_hash, description, entry,
                                         links, tools, updated_at)
                 VALUES (?1, ?2, 1, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT (name) DO UPDATE SET
                     version = version + (content_hash != excluded.content_hash),
                     updated_at = CASE WHEN content_hash != excluded.content_hash
                                       THEN excluded.updated_at ELSE updated_at END,
                     path = excluded.path,
                     content_hash = excluded.content_hash,
                     description = excluded.description,
                     entry = excluded.entry,
                     links = excluded.links,
                     tools = excluded.tools",
            )
            .bind(&identity.name)
            .bind(&identity.path)
            .bind(&identity.content_hash)
            .bind(&identity.description)
            .bind(identity.entry)
            .bind(serde_json::to_string(&identity.links)?)
            .bind(tools)
            .bind(now)
            .execute(&mut *transaction)
            .await
            .context("store identity")?;
        }
        let names: Vec<&str> = found
            .iter()
            .map(|identity| identity.name.as_str())
            .collect();
        sqlx::query("DELETE FROM identities WHERE name NOT IN (SELECT value FROM json_each(?))")
            .bind(serde_json::to_string(&names)?)
            .execute(&mut *transaction)
            .await
            .context("drop old identities")?;
        transaction.commit().await.context("commit identity sync")?;
        self.identities().await
    }

    pub async fn identities(&self) -> Result<Vec<Identity>> {
        sqlx::query(
            "SELECT name, path, version, content_hash, description, entry, links, tools,
                    updated_at
             FROM identities ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .context("list identities")?
        .into_iter()
        .map(|row| {
            let links: String = row.try_get("links")?;
            let tools: Option<String> = row.try_get("tools")?;
            let entry: i64 = row.try_get("entry")?;
            Ok(Identity {
                name: row.try_get("name")?,
                path: row.try_get("path")?,
                version: row.try_get("version")?,
                content_hash: row.try_get("content_hash")?,
                description: row.try_get("description")?,
                entry: entry == 1,
                links: serde_json::from_str(&links).context("identity links")?,
                tools: tools
                    .map(|tools| serde_json::from_str(&tools))
                    .transpose()
                    .context("identity tools")?,
                updated_at: row.try_get("updated_at")?,
            })
        })
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::Identity;
    use crate::testing::temp_state;

    fn identity(name: &str, hash: &str) -> Identity {
        Identity {
            name: name.into(),
            path: format!("/agents/{name}/AGENT.md"),
            version: 0,
            content_hash: hash.into(),
            description: "reviews code".into(),
            entry: true,
            links: vec![],
            tools: None,
            updated_at: 0,
        }
    }

    #[tokio::test]
    async fn a_changed_file_raises_the_version() {
        let state = temp_state("identities").await;
        let found = [identity("reviewer", "a"), identity("writer", "b")];
        let synced = state.sync_identities(&found).await.unwrap();
        assert!(synced.iter().all(|identity| identity.version == 1));

        let found = [identity("reviewer", "a2")];
        let synced = state.sync_identities(&found).await.unwrap();
        assert_eq!(synced.len(), 1, "a removed agent is dropped");
        assert_eq!(synced[0].version, 2);
        let synced = state.sync_identities(&found).await.unwrap();
        assert_eq!(synced[0].version, 2, "an unchanged file keeps its version");
    }
}
