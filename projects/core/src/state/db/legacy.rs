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

/// The migrations of the task tables before steps got timers.
const TASK_TABLES: [i64; 2] = [8, 9];
/// What they make now, in order.
const TASK_TABLES_SQL: [&str; 2] = [
    include_str!("migrations/0008_tasks.sql"),
    include_str!("migrations/0009_task_items.sql"),
];

/// Databases made before steps had timers and tasks listed their steps. Both tables are
/// built again from the migrations now and keep their data; a step done back then counts
/// as started and finished when its task was made.
pub async fn convert_tasks(pool: &SqlitePool, migrator: &Migrator) -> Result<()> {
    let ledger: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master
                        WHERE type = 'table' AND name = '_sqlx_migrations')",
    )
    .fetch_one(pool)
    .await
    .context("look for the migration record")?;
    if !ledger {
        return Ok(());
    }
    let recorded: Option<Vec<u8>> =
        sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version = ?")
            .bind(TASK_TABLES[0])
            .fetch_optional(pool)
            .await
            .context("read the task migration record")?;
    let migrations: Vec<_> = migrator
        .iter()
        .filter(|migration| TASK_TABLES.contains(&migration.version))
        .collect();
    ensure!(migrations.len() == 2, "no task migrations");
    let Some(recorded) = recorded else {
        return Ok(());
    };
    if recorded[..] == migrations[0].checksum[..] {
        return Ok(());
    }
    let mut transaction = pool.begin().await.context("begin task upgrade")?;
    sqlx::raw_sql(
        "DROP INDEX tasks_thread;
         ALTER TABLE task_items RENAME TO old_task_items;
         ALTER TABLE tasks RENAME TO old_tasks;",
    )
    .execute(&mut *transaction)
    .await
    .context("set the old task tables aside")?;
    for (version, sql) in TASK_TABLES.iter().zip(TASK_TABLES_SQL) {
        sqlx::raw_sql(sql)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("apply migration {version}"))?;
    }
    sqlx::raw_sql(
        "INSERT INTO tasks (task_id, thread_id, description, item_ids, done, created_at,
                            updated_at)
         SELECT task_id, thread_id, description,
                (SELECT json_group_array(item_id) FROM
                    (SELECT item_id FROM old_task_items i
                     WHERE i.task_id = t.task_id ORDER BY position)),
                done, created_at, created_at
         FROM old_tasks t;
         INSERT INTO task_items (item_id, task_id, description, done, created_at, updated_at,
                                 started_at, finished_at)
         SELECT i.item_id, i.task_id, i.description, i.done, t.created_at, t.created_at,
                CASE WHEN i.done = 1 THEN t.created_at END,
                CASE WHEN i.done = 1 THEN t.created_at END
         FROM old_task_items i JOIN old_tasks t USING (task_id);
         DROP TABLE old_task_items;
         DROP TABLE old_tasks;",
    )
    .execute(&mut *transaction)
    .await
    .context("move the tasks over")?;
    for migration in &migrations {
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ?, description = ? WHERE version = ?")
            .bind(migration.checksum.as_ref())
            .bind(migration.description.as_ref())
            .bind(migration.version)
            .execute(&mut *transaction)
            .await
            .context("record a task migration")?;
    }
    transaction.commit().await.context("commit task upgrade")?;
    Ok(())
}

/// What the messages migration adds now to a table made before image cards.
const MESSAGE_ARTIFACTS: &str = "ALTER TABLE messages ADD COLUMN artifacts_json TEXT
    CHECK (artifacts_json IS NULL OR (role = 'tool'
        AND json_valid(artifacts_json) AND json_type(artifacts_json) = 'array'));";
/// A development build added the column in a migration of its own.
const OLD_ARTIFACTS_MIGRATION: &str = "image artifacts";
/// What the messages migration adds now to a table made before background jobs.
const MESSAGE_JOBS: &str = "ALTER TABLE messages ADD COLUMN from_job_id TEXT
    CHECK (from_job_id IS NULL OR role = 'system');
    CREATE UNIQUE INDEX IF NOT EXISTS messages_job ON messages(from_job_id)
        WHERE from_job_id IS NOT NULL;";

/// Databases made before messages carried image cards get the column, and the record
/// of the messages migration is brought to its text now.
pub async fn convert_messages(pool: &SqlitePool, migrator: &Migrator) -> Result<()> {
    let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('messages')")
        .fetch_all(pool)
        .await
        .context("read the message columns")?;
    if columns.is_empty() {
        return Ok(());
    }
    let migration = migrator
        .iter()
        .find(|migration| migration.version == 2)
        .context("no messages migration")?;
    let mut transaction = pool.begin().await.context("begin message upgrade")?;
    if !columns.iter().any(|column| column == "artifacts_json") {
        sqlx::raw_sql(MESSAGE_ARTIFACTS)
            .execute(&mut *transaction)
            .await
            .context("add the image card column")?;
    }
    if !columns.iter().any(|column| column == "from_job_id") {
        sqlx::raw_sql(MESSAGE_JOBS)
            .execute(&mut *transaction)
            .await
            .context("add the job result column")?;
    }
    sqlx::query("DELETE FROM _sqlx_migrations WHERE description = ?")
        .bind(OLD_ARTIFACTS_MIGRATION)
        .execute(&mut *transaction)
        .await
        .context("drop the old image card migration")?;
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 2")
        .bind(migration.checksum.as_ref())
        .execute(&mut *transaction)
        .await
        .context("record the messages migration")?;
    transaction
        .commit()
        .await
        .context("commit message upgrade")?;
    Ok(())
}

/// What the pending messages migration adds now to a table made before background jobs.
const PENDING_JOBS: &str = "ALTER TABLE pending_messages ADD COLUMN from_job_id TEXT;
    CREATE UNIQUE INDEX IF NOT EXISTS pending_messages_job ON pending_messages(from_job_id)
        WHERE from_job_id IS NOT NULL;";
/// What the jobs migration holds now beyond the first build of the table.
const JOB_OWNERS: &str = "ALTER TABLE background_jobs ADD COLUMN owner_runtime_id TEXT;
    ALTER TABLE background_jobs ADD COLUMN owner_lease_path TEXT;
    CREATE INDEX IF NOT EXISTS background_jobs_runtime
        ON background_jobs(owner_runtime_id, status);";
/// A development build kept the owner columns in a migration of its own.
const OLD_OWNERS_MIGRATION: &str = "background job owners";

/// Databases made before background jobs get the job result column of the pending
/// messages, and ones from a development build the owner columns of the jobs; the records
/// of the pending messages and jobs migrations are brought to their text now.
pub async fn convert_jobs(pool: &SqlitePool, migrator: &Migrator) -> Result<()> {
    let columns = |table: &'static str| async move {
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?)")
            .bind(table)
            .fetch_all(pool)
            .await
            .context("read the table columns")
    };
    let pending = columns("pending_messages").await?;
    if pending.is_empty() {
        return Ok(());
    }
    let jobs = columns("background_jobs").await?;
    let checksum = |version: i64| {
        migrator
            .iter()
            .find(|migration| migration.version == version)
            .map(|migration| migration.checksum.to_vec())
            .context("a migration is missing")
    };
    let mut transaction = pool.begin().await.context("begin jobs upgrade")?;
    if !pending.iter().any(|column| column == "from_job_id") {
        sqlx::raw_sql(PENDING_JOBS)
            .execute(&mut *transaction)
            .await
            .context("add the job result column to pending messages")?;
    }
    sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 7")
        .bind(checksum(7)?)
        .execute(&mut *transaction)
        .await
        .context("record the pending messages migration")?;
    if !jobs.is_empty() {
        if !jobs.iter().any(|column| column == "owner_runtime_id") {
            sqlx::raw_sql(JOB_OWNERS)
                .execute(&mut *transaction)
                .await
                .context("add the job owner columns")?;
        }
        // Matched by name as well: a later migration may take that number.
        sqlx::query("DELETE FROM _sqlx_migrations WHERE version = 14 AND description = ?")
            .bind(OLD_OWNERS_MIGRATION)
            .execute(&mut *transaction)
            .await
            .context("drop the old job owners migration")?;
        sqlx::query("UPDATE _sqlx_migrations SET checksum = ? WHERE version = 13")
            .bind(checksum(13)?)
            .execute(&mut *transaction)
            .await
            .context("record the jobs migration")?;
    }
    transaction.commit().await.context("commit jobs upgrade")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{state::State, testing::temp_dir};

    #[tokio::test]
    async fn messages_without_image_cards_get_the_column() {
        let path = temp_dir("legacy-messages").join("state.db");
        State::open(&path).await.unwrap().close().await;
        for old in [
            // Made before image cards.
            "ALTER TABLE messages DROP COLUMN artifacts_json;
             UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 2;",
            // A development build with the column in a migration of its own.
            "DROP TABLE goals;
             DROP TABLE background_jobs;
             DROP TABLE thread_projects;
             DELETE FROM _sqlx_migrations WHERE version >= 12;
             UPDATE _sqlx_migrations SET checksum = X'00' WHERE version = 2;
             INSERT INTO _sqlx_migrations (version, description, success, checksum,
                execution_time) VALUES (12, 'image artifacts', TRUE, X'00', 0);",
        ] {
            let state = State::open(&path).await.unwrap();
            sqlx::raw_sql(old).execute(&state.pool).await.unwrap();
            state.close().await;
            let state = State::open(&path).await.unwrap();
            let column: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM pragma_table_info('messages') WHERE name = 'artifacts_json'",
            )
            .fetch_one(&state.pool)
            .await
            .unwrap();
            assert_eq!(column, 1);
            state.close().await;
        }
    }

    #[tokio::test]
    async fn a_database_before_jobs_gets_their_columns() {
        let path = temp_dir("legacy-before-jobs").join("state.db");
        State::open(&path).await.unwrap().close().await;
        let state = State::open(&path).await.unwrap();
        // As a release without background jobs left it: no job columns or table, and
        // the messages and pending messages migrations as they were.
        sqlx::raw_sql(
            "DROP INDEX messages_job;
             DROP INDEX pending_messages_job;
             ALTER TABLE messages DROP COLUMN from_job_id;
             ALTER TABLE pending_messages DROP COLUMN from_job_id;
             DROP TABLE background_jobs;
             DROP TABLE thread_projects;
             DELETE FROM _sqlx_migrations WHERE version >= 13;
             UPDATE _sqlx_migrations SET checksum = X'00' WHERE version IN (2, 7);",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        state.close().await;
        let state = State::open(&path).await.unwrap();
        for (table, column) in [
            ("messages", "from_job_id"),
            ("pending_messages", "from_job_id"),
            ("background_jobs", "owner_runtime_id"),
        ] {
            let found: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
                    .bind(table)
                    .bind(column)
                    .fetch_one(&state.pool)
                    .await
                    .unwrap();
            assert_eq!(found, 1, "{table}.{column}");
        }
        state.close().await;
    }

    #[tokio::test]
    async fn a_development_build_of_jobs_loses_its_owner_migration() {
        let path = temp_dir("legacy-job-owners").join("state.db");
        State::open(&path).await.unwrap().close().await;
        let state = State::open(&path).await.unwrap();
        // The first jobs build recorded the owners as a migration of its own.
        sqlx::raw_sql(
            "DROP TABLE thread_projects;
             DELETE FROM _sqlx_migrations WHERE version = 14;
             UPDATE _sqlx_migrations SET checksum = X'00' WHERE version IN (2, 7, 13);
             INSERT INTO _sqlx_migrations (version, description, success, checksum,
                execution_time) VALUES (14, 'background job owners', TRUE, X'00', 0);",
        )
        .execute(&state.pool)
        .await
        .unwrap();
        state.close().await;
        let state = State::open(&path).await.unwrap();
        // Its number went to the next migration, which ran in its place.
        let description: String =
            sqlx::query_scalar("SELECT description FROM _sqlx_migrations WHERE version = 14")
                .fetch_one(&state.pool)
                .await
                .unwrap();
        assert_ne!(description, super::OLD_OWNERS_MIGRATION);
        state.close().await;
    }
}
