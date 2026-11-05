//! Writing an archive: walks the data roots into a manifest and streams the files out.

use std::{
    fs::{self, File},
    io::{self, BufWriter, ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use sqlx::{
    ConnectOptions, Connection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use uuid::Uuid;

use crate::{init::paths::Paths, state::State};

use super::{
    BACKUPS, FORMAT, MAGIC, MAX_MANIFEST_BYTES, VERSION,
    files::{Temporary, private_dir, private_file, sync_directory},
    inventory::{Inventory, collect},
    manifest::{Kind, Manifest, validate_manifest},
    verify::{directory_tree, inspect_tree_path},
};

/// Export only the standard database. A custom database needs an explicit migration
/// policy; silently backing up the unused default would lose history on the new machine.
pub async fn export(
    paths: &Paths,
    state: &State,
    state_db: &Path,
    destination: Option<PathBuf>,
) -> Result<PathBuf> {
    inspect_tree_path(paths.config_root())?;
    inspect_tree_path(paths.workspace_root())?;
    inspect_tree_path(&paths.state_db())?;
    let actual = fs::canonicalize(state_db).context("resolve the active state database")?;
    let standard =
        fs::canonicalize(paths.state_db()).context("resolve the standard state database")?;
    ensure!(
        actual == standard,
        "/migrate cannot export a custom --state-db database; close duskcode and restart without --state-db using the standard database before exporting. The custom database has not been changed"
    );
    let backup_root = paths.workspace_root().join(BACKUPS);
    directory_tree(&backup_root)?;
    let destination = destination
        .unwrap_or_else(|| backup_root.join(format!("duskcode-{}.duskarchive", Uuid::now_v7())));
    let destination = if destination.is_absolute() {
        destination
    } else {
        std::env::current_dir()?.join(destination)
    };
    inspect_tree_path(&destination)?;
    ensure!(
        destination.starts_with(&backup_root) && destination != backup_root,
        "/migrate output must be inside {}; that backup directory is excluded from archives",
        backup_root.display()
    );
    let parent = destination
        .parent()
        .context("archive has no parent directory")?;
    directory_tree(parent)?;
    ensure!(
        fs::symlink_metadata(&destination).is_err_and(|error| error.kind() == ErrorKind::NotFound),
        "archive output already exists or cannot be inspected; choose a new filename"
    );
    let temporary = Temporary::new(&backup_root, "export")?;
    let snapshot = temporary.path.join("state.db");
    // SQLite permits an empty destination. Pre-create it with 0600 rather than allowing
    // SQLite to choose permissions, and keep it behind the private staging directory.
    private_file(&snapshot)?.sync_all()?;
    let snapshot_name = snapshot.to_str().context("snapshot path is not UTF-8")?;
    sqlx::query("VACUUM INTO ?")
        .bind(snapshot_name)
        .execute(&state.pool)
        .await
        .context(
            "create a consistent SQLite snapshot for /migrate; retry when database writes are idle",
        )?;
    rebuild_snapshot_search(&snapshot).await?;
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || export_files(&paths, &snapshot, &destination, temporary))
        .await
        .context("archive writer stopped")?
}

/// `messages.message_id` is a TEXT primary key, so its implicit rowids may change
/// during VACUUM. Rebuild the external-content FTS index against the snapshot's own
/// messages before copying it. The live database and its search index are untouched.
pub async fn rebuild_snapshot_search(snapshot: &Path) -> Result<()> {
    let options = SqliteConnectOptions::new()
        .filename(snapshot)
        .create_if_missing(false)
        // The archived snapshot must be self-contained after this connection closes.
        .journal_mode(SqliteJournalMode::Delete)
        .disable_statement_logging();
    let mut connection = sqlx::SqliteConnection::connect_with(&options)
        .await
        .context("open the migration snapshot to rebuild message search")?;
    let rebuilt = sqlx::query("INSERT INTO messages_search(messages_search) VALUES ('rebuild')")
        .execute(&mut connection)
        .await
        .context("rebuild message search against the migration snapshot's rowids");
    // Close even after a rebuild error, before export cleanup removes the staging tree.
    let closed = connection
        .close()
        .await
        .context("close the rebuilt migration snapshot");
    rebuilt?;
    closed?;
    Ok(())
}

pub fn export_files(
    paths: &Paths,
    snapshot: &Path,
    destination: &Path,
    temporary: Temporary,
) -> Result<PathBuf> {
    let data = temporary.path.join("data");
    private_dir(&data)?;
    let mut inventory = Inventory::default();
    collect(
        paths.config_root(),
        &data,
        "config",
        paths,
        snapshot,
        &mut inventory,
    )?;
    collect(
        paths.workspace_root(),
        &data,
        "workspace",
        paths,
        snapshot,
        &mut inventory,
    )?;
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        application_version: env!("CARGO_PKG_VERSION").into(),
        created_at_ms: crate::state::timestamp_ms()?,
        excluded_workspace_paths: vec![BACKUPS.into()],
        entries: inventory.entries,
    };
    validate_manifest(&manifest)?;
    let json = serde_json::to_vec(&manifest)?;
    ensure!(
        json.len() as u64 <= MAX_MANIFEST_BYTES,
        "archive manifest is too large"
    );
    let archive_path = temporary.path.join("archive");
    let mut output = BufWriter::new(private_file(&archive_path)?);
    output.write_all(MAGIC)?;
    output.write_all(&(json.len() as u64).to_le_bytes())?;
    output.write_all(&json)?;
    for entry in &manifest.entries {
        if entry.kind == Kind::File {
            let mut input = File::open(data.join(&entry.path))?;
            ensure!(
                io::copy(&mut input, &mut output)? == entry.size,
                "staged file changed"
            );
        }
    }
    output.flush()?;
    output.get_ref().sync_all()?;
    // Keep the original writable handle until the published inode is fully synced.
    // Unlike rename, creating a hard link never replaces a concurrently created output.
    // Staging and output are both inside backups, so they are on the same filesystem.
    fs::hard_link(&archive_path, destination)
        .context("publish the archive without overwriting an existing file")?;
    let parent = destination
        .parent()
        .context("published archive has no parent")?;
    sync_directory(parent).with_context(|| {
        format!(
            "archive was published at {}, but its directory could not be synced",
            destination.display()
        )
    })?;
    // Remove this temporary hard link explicitly so the final inode/link count and
    // destination name can be synced before reporting a successful durable publish.
    fs::remove_file(&archive_path).with_context(|| {
        format!(
            "archive was published at {}, but its temporary link could not be removed",
            destination.display()
        )
    })?;
    output.get_ref().sync_all()?;
    sync_directory(parent)?;
    sync_directory(&temporary.path)?;
    Ok(destination.to_owned())
}
