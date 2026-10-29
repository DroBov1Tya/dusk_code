//! Databases made before there was one migration per table. They recorded up to ten
//! migrations, the last three adding columns to tables the first seven made. Such a
//! database gets the columns it still lacks, so its schema matches the migrations now,
//! and its migration record is rewritten to them. Its data stays as it is.

use anyhow::{Context, Result, ensure};
use sqlx::{SqlitePool, migrate::Migrator};

/// What the old migrations 8 to 10 added, by their version.
const OLD_CHANGES: [(i64, &str); 3] = [
    (
        8,
        "ALTER TABLE threads ADD COLUMN parent_thread_id TEXT REFERENCES threads(id);
         ALTER TABLE threads ADD COLUMN agent TEXT CHECK (agent IS NULL OR length(trim(agent)) > 0);
         CREATE INDEX threads_parent ON threads(parent_thread_id);",
    ),
    (
        9,
        "ALTER TABLE pending_messages ADD COLUMN from_thread_id TEXT REFERENCES threads(id);
         ALTER TABLE messages ADD COLUMN from_thread_id TEXT REFERENCES threads(id)
             CHECK (from_thread_id IS NULL OR role = 'user');",
    ),
    (
        10,
        "ALTER TABLE threads ADD COLUMN name TEXT CHECK (name IS NULL OR length(trim(name)) > 0);
         CREATE UNIQUE INDEX threads_parent_name ON threads(parent_thread_id, name)
             WHERE name IS NOT NULL;",
    ),
];
/// The migrations now that make the tables the old ten did.
const TABLES_THROUGH: i64 = 7;

/// Does nothing to a new database or one made with the migrations now.
pub async fn convert(pool: &SqlitePool, migrator: &Migrator) -> Result<()> {
    let recorded: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await
    .context("look for the migration record")?;
    if !recorded {
        return Ok(());
    }
    let applied: Vec<(i64, Vec<u8>)> =
        sqlx::query_as("SELECT version, checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(pool)
            .await
            .context("read the migration record")?;
    let first = migrator
        .iter()
        .find(|migration| migration.version == 1)
        .context("no first migration")?;
    // The first migration changed when the tables were folded, so its checksum tells.
    let old = applied
        .iter()
        .any(|(version, checksum)| *version == 1 && checksum[..] != first.checksum[..]);
    if !old {
        return Ok(());
    }
    let last = applied.last().map_or(0, |(version, _)| *version);
    ensure!(
        last >= TABLES_THROUGH,
        "the state database is older than this version can upgrade"
    );
    let mut transaction = pool.begin().await.context("begin upgrade")?;
    for (version, change) in OLD_CHANGES.iter().filter(|(version, _)| *version > last) {
        sqlx::raw_sql(*change)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("apply old migration {version}"))?;
    }
    sqlx::query("DELETE FROM _sqlx_migrations")
        .execute(&mut *transaction)
        .await
        .context("clear the migration record")?;
    for migration in migrator
        .iter()
        .filter(|migration| migration.version <= TABLES_THROUGH)
    {
        sqlx::query(
            "INSERT INTO _sqlx_migrations (version, description, success, checksum,
                execution_time)
             VALUES (?, ?, TRUE, ?, 0)",
        )
        .bind(migration.version)
        .bind(migration.description.as_ref())
        .bind(migration.checksum.as_ref())
        .execute(&mut *transaction)
        .await
        .context("record a migration")?;
    }
    transaction.commit().await.context("commit upgrade")?;
    Ok(())
}
