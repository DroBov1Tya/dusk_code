//! Private portable archives. Version 1 is an uncompressed stream: magic, a little-endian
//! u64 JSON-manifest length, the manifest, then regular-file contents in manifest order.
//! No executable archive parser or external archiver is involved. Links and special files
//! are refused rather than followed; saved paths inside file contents are not rewritten.

use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, File, Metadata, OpenOptions, TryLockError},
    io::{self, BufReader, BufWriter, ErrorKind, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    ConnectOptions, Connection,
    sqlite::{SqliteConnectOptions, SqliteJournalMode},
};
use uuid::Uuid;

use crate::{init::paths::Paths, state::State};

const MAGIC: &[u8] = b"DUSKCODE-ARCHIVE\n";
const FORMAT: &str = "duskcode-portable";
const VERSION: u32 = 1;
const BACKUPS: &str = "backups";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;
const PENDING_RESTORE: &str = ".restore-pending";
const RESTORE_LOCK: &str = ".restore.lock";
const MAX_TRANSACTION_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    application_version: String,
    created_at_ms: i64,
    /// Backups are deliberately not backed up again.
    excluded_workspace_paths: Vec<String>,
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Only `config[/...]` or `workspace[/...]`, with portable slash-separated names.
    path: String,
    kind: Kind,
    size: u64,
    sha256: Option<String>,
    executable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Directory,
    File,
}

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
async fn rebuild_snapshot_search(snapshot: &Path) -> Result<()> {
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

fn export_files(
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
        created_at_ms: crate::state::db_timestamp()?,
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

fn collect(
    source: &Path,
    data: &Path,
    name: &str,
    paths: &Paths,
    snapshot: &Path,
    inventory: &mut Inventory,
) -> Result<()> {
    validate_path(name)?;
    let metadata = fs::symlink_metadata(source)
        .with_context(|| format!("inspect archive source {}", source.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "cannot archive symlink {}; replace it with real data before /migrate",
        source.display()
    );
    let target = data.join(name);
    if metadata.is_dir() {
        private_dir(&target)?;
        let entry = Entry {
            path: name.into(),
            kind: Kind::Directory,
            size: 0,
            sha256: None,
            executable: false,
        };
        inventory.reserve(&entry)?;
        inventory.entries.push(entry);
        for child in fs::read_dir(source)? {
            let child = child?;
            let child_path = child.path();
            if child_path == paths.workspace_root().join(BACKUPS)
                || database_sidecar(&child_path, &paths.state_db())
            {
                continue;
            }
            let child_name = child
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("archive source has a non-UTF-8 filename"))?;
            let next_name = format!("{name}/{child_name}");
            if child_path == paths.state_db() {
                collect_file(snapshot, &data.join(&next_name), &next_name, inventory)?;
            } else {
                collect(&child_path, data, &next_name, paths, snapshot, inventory)?;
            }
        }
    } else {
        ensure!(
            metadata.is_file(),
            "cannot archive special file {}",
            source.display()
        );
        collect_file(source, &target, name, inventory)?;
    }
    Ok(())
}

fn collect_file(source: &Path, target: &Path, name: &str, inventory: &mut Inventory) -> Result<()> {
    validate_path(name)?;
    inspect_tree_path(source)?;
    let before = fs::symlink_metadata(source)?;
    ensure!(before.is_file(), "archive source is not a regular file");
    let mut input = open_regular_input(source)?;
    ensure_same_file(&before, &input.metadata()?)?;
    let size = before.len();
    ensure!(
        size <= MAX_PAYLOAD_BYTES,
        "archive source file is too large"
    );
    let mut entry = Entry {
        path: name.into(),
        kind: Kind::File,
        size,
        sha256: Some("0".repeat(64)),
        executable: executable(&before),
    };
    inventory.reserve(&entry)?;
    let mut output = private_file(target)?;
    let (copied, hash) = copy_hash(&mut Read::by_ref(&mut input).take(size + 1), &mut output)?;
    ensure!(
        copied == size,
        "{} changed during /migrate; retry when file writes are idle",
        source.display()
    );
    output.sync_all()?;
    entry.sha256 = Some(hash);
    inventory.entries.push(entry);
    Ok(())
}

#[derive(Default)]
struct Inventory {
    entries: Vec<Entry>,
    payload_bytes: u64,
    entry_json_bytes: u64,
}

impl Inventory {
    /// Reserve bounds before copying another source file. Account for the JSON representation
    /// of each entry, plus room for the fixed manifest fields, so inventory RAM stays bounded.
    fn reserve(&mut self, entry: &Entry) -> Result<()> {
        ensure!(
            self.entries.len() < MAX_ENTRIES,
            "archive exceeds the 100,000-entry limit"
        );
        self.payload_bytes = self
            .payload_bytes
            .checked_add(entry.size)
            .context("archive payload size overflow")?;
        ensure!(
            self.payload_bytes <= MAX_PAYLOAD_BYTES,
            "archive exceeds the 1 TiB payload limit"
        );
        self.entry_json_bytes += serde_json::to_vec(entry)?.len() as u64 + 1;
        ensure!(
            self.entry_json_bytes <= MAX_MANIFEST_BYTES - 1024,
            "archive exceeds the 16 MiB manifest limit"
        );
        Ok(())
    }
}

fn database_sidecar(path: &Path, database: &Path) -> bool {
    ["-wal", "-shm", "-journal"].iter().any(|suffix| {
        let mut name = database.as_os_str().to_owned();
        name.push(suffix);
        path == Path::new(&name)
    })
}

/// Nothing in the application is initialized here. Validate the entire manifest and
/// payload, in private staging, before installing either root. Other Dusk processes must
/// be closed: there is no lock shared with already-running releases.
pub fn restore(paths: &Paths, archive: &Path) -> Result<()> {
    let _guard = restore_guard(true)?;
    require_no_pending_restore()?;
    preflight(paths.config_root())?;
    preflight(paths.workspace_root())?;
    ensure!(
        !paths.config_root().starts_with(paths.workspace_root())
            && !paths.workspace_root().starts_with(paths.config_root()),
        "restore roots must be separate directories"
    );
    let mut input = BufReader::new(open_regular_input(archive)?);
    let archive_bytes = input.get_ref().metadata()?.len();
    ensure!(
        input.get_ref().metadata()?.is_file(),
        "migration archive must be a regular file"
    );
    let mut magic = [0; MAGIC.len()];
    input
        .read_exact(&mut magic)
        .context("read archive header")?;
    ensure!(
        magic.as_slice() == MAGIC,
        "not a duskcode migration archive"
    );
    let mut length = [0; 8];
    input
        .read_exact(&mut length)
        .context("read archive manifest length")?;
    let length = u64::from_le_bytes(length);
    ensure!(
        length > 0 && length <= MAX_MANIFEST_BYTES,
        "invalid archive manifest size"
    );
    let mut json = vec![0; length as usize];
    input
        .read_exact(&mut json)
        .context("read archive manifest")?;
    let manifest: Manifest = serde_json::from_slice(&json).context("invalid archive manifest")?;
    let payload_bytes = validate_manifest(&manifest)?;
    let expected_bytes = (MAGIC.len() as u64)
        .checked_add(8)
        .and_then(|n| n.checked_add(length))
        .and_then(|n| n.checked_add(payload_bytes))
        .context("archive size overflow")?;
    ensure!(
        expected_bytes == archive_bytes,
        "archive is truncated or has trailing data"
    );
    let config_parent = paths
        .config_root()
        .parent()
        .context("config root has no parent")?;
    let workspace_parent = paths
        .workspace_root()
        .parent()
        .context("workspace root has no parent")?;
    directory_tree(config_parent)?;
    directory_tree(workspace_parent)?;
    let mut config = Temporary::new(config_parent, "restore-config")?;
    let mut workspace = Temporary::new(workspace_parent, "restore-workspace")?;
    for entry in &manifest.entries {
        let (root, relative) = entry.path.split_once('/').unwrap_or((&entry.path, ""));
        let target = match root {
            "config" => config.path.join("root"),
            "workspace" => workspace.path.join("root"),
            _ => unreachable!("validated root"),
        }
        .join(relative);
        if entry.kind == Kind::Directory {
            private_dir(&target)?;
        } else {
            let mut output = private_file(&target)?;
            let (copied, hash) =
                copy_hash(&mut Read::by_ref(&mut input).take(entry.size), &mut output)?;
            ensure!(copied == entry.size, "archive payload is truncated");
            ensure!(
                Some(&hash) == entry.sha256.as_ref(),
                "archive checksum mismatch for {}",
                entry.path
            );
            set_executable(&output, entry.executable)?;
            output.sync_all()?;
        }
    }
    let mut end = [0];
    ensure!(input.read(&mut end)? == 0, "archive has trailing data");
    preflight(paths.config_root())?;
    preflight(paths.workspace_root())?;
    sync_staging(&config.path, "config", &manifest)?;
    sync_staging(&workspace.path, "workspace", &manifest)?;
    // A published journal owns these directories. Never let an error-path Drop discard
    // them once a root may have been installed; explicit recovery validates them first.
    config.keep = true;
    workspace.keep = true;
    if let Err(error) = publish_transaction(paths, &config.path, &workspace.path, &json) {
        if fs::symlink_metadata(pending_restore_path()?)
            .is_err_and(|error| error.kind() == ErrorKind::NotFound)
        {
            config.keep = false;
            workspace.keep = false;
        }
        return Err(error).with_context(|| {
            format!(
                "cannot journal restore; preserve any retained staging at {} and {}. If a pending journal was published, startup is blocked until duskcode restore --recover succeeds",
                config.path.display(),
                workspace.path.display()
            )
        });
    }
    finish_install(paths, &manifest, &mut config, &mut workspace)
}

/// A live runtime holds a shared OS lock; offline restore and recovery require the
/// exclusive lock. The lock file is never unlinked, so SIGKILL only releases ownership.
pub struct RestoreGuard {
    _file: File,
}

/// The HOME-scoped transaction only owns the standard data roots. Sandboxed runtimes
/// with disjoint roots must not create default state or depend on its pending journal.
pub fn startup_guard(paths: &Paths, state_db: Option<&Path>) -> Result<Option<RestoreGuard>> {
    // Resolve before the HOME decision as well: an unresolved relative DB must never
    // silently bypass coordination merely because current_dir could not be obtained.
    let state_db = state_db.map(resolve_state_db).transpose()?;
    let config_root = absolute_for_overlap(paths.config_root())?;
    let workspace_root = absolute_for_overlap(paths.workspace_root())?;
    let standard = match Paths::from_home() {
        Ok(standard) => standard,
        Err(_) => return Ok(None),
    };
    let shared = [
        Some(config_root.as_path()),
        Some(workspace_root.as_path()),
        state_db.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|actual| {
        [standard.config_root(), standard.workspace_root()]
            .iter()
            .any(|default| paths_overlap(actual, default))
    });
    if !shared {
        return Ok(None);
    }
    let guard = restore_guard(false)?;
    require_no_pending_restore()?;
    Ok(Some(guard))
}

/// Pin database selection before startup can await or initialize other components.
/// The caller passes this same absolute path to both coordination and State::open.
pub fn resolve_state_db(path: &Path) -> Result<PathBuf> {
    ensure!(!path.as_os_str().is_empty(), "state database path is empty");
    absolute_for_overlap(path)
}

fn absolute_for_overlap(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Ok(std::env::current_dir()
        .context(
            "cannot resolve a relative data path because the current directory is unavailable",
        )?
        .join(path))
}

fn paths_overlap(left: &Path, right: &Path) -> bool {
    if left.starts_with(right) || right.starts_with(left) {
        return true;
    }
    match (physical_path(left), physical_path(right)) {
        (Some(left), Some(right)) => left.starts_with(&right) || right.starts_with(&left),
        _ => false,
    }
}

/// Resolve existing ancestors too: a symlink alias must still coordinate when the
/// final data root has not been created yet. A disjoint inaccessible HOME is harmless.
fn physical_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if let Ok(mut physical) = fs::canonicalize(ancestor) {
            // Preserve symlink semantics in the existing prefix, then normalize the
            // missing suffix. A missing `sub/../state.db` still belongs to its parent.
            for component in path.strip_prefix(ancestor).ok()?.components() {
                match component {
                    Component::Normal(name) => physical.push(name),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        physical.pop();
                    }
                    Component::Prefix(_) | Component::RootDir => return None,
                }
            }
            return Some(physical);
        }
    }
    None
}

/// Removal uses the same exclusive lock as restore. The command must retain this
/// guard through all deletions and must leave the coordination inode linked in place.
pub struct RemovalGuard {
    guard: RestoreGuard,
    lock_path: PathBuf,
}

pub fn removal_guard() -> Result<RemovalGuard> {
    let guard = restore_guard(true)?;
    require_no_pending_restore()?;
    Ok(RemovalGuard {
        guard,
        lock_path: restore_control_root()?.join(RESTORE_LOCK),
    })
}

impl RemovalGuard {
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    pub fn validate_application_data_root(&self, directory: &Path) -> Result<()> {
        self.removal_directory(directory).map(|_| ())
    }

    /// A default install shares the restore control directory. Delete its other
    /// children, never that directory or the live lock, so another process cannot
    /// open a replacement inode while this exclusive owner is still deleting data.
    pub fn remove_application_data(&self, directory: &Path) -> Result<()> {
        require_no_pending_restore()?;
        let Some((directory, control)) = self.removal_directory(directory)? else {
            return Ok(());
        };
        if directory == control {
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                if entry.file_name() == RESTORE_LOCK {
                    continue;
                }
                ensure!(
                    entry.file_name() != PENDING_RESTORE,
                    "pending restore journal appeared; removal stopped without deleting it"
                );
                let metadata = fs::symlink_metadata(entry.path())?;
                if metadata.is_dir() {
                    fs::remove_dir_all(entry.path())?;
                } else {
                    fs::remove_file(entry.path())?;
                }
            }
            sync_directory(&control)?;
        } else {
            fs::remove_dir_all(&directory)
                .with_context(|| format!("cannot remove {}", directory.display()))?;
        }
        Ok(())
    }

    fn removal_directory(&self, directory: &Path) -> Result<Option<(PathBuf, PathBuf)>> {
        inspect_tree_path(directory)?;
        match fs::symlink_metadata(directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir(),
                "application data root must be a real directory"
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("inspect application data before removal"),
        }
        // Confirm the pathname still names the held inode before allowing deletion.
        ensure_same_file(
            &fs::symlink_metadata(&self.lock_path)?,
            &self.guard._file.metadata()?,
        )?;
        let control = fs::canonicalize(
            self.lock_path
                .parent()
                .context("restore lock has no parent")?,
        )?;
        let directory = fs::canonicalize(directory)?;
        ensure!(
            directory == control || !control.starts_with(&directory),
            "refusing to recursively remove an application data root containing the restore control directory"
        );
        Ok(Some((directory, control)))
    }
}

fn restore_guard(exclusive: bool) -> Result<RestoreGuard> {
    let root = restore_control_root()?;
    directory_tree(&root)?;
    let path = root.join(RESTORE_LOCK);
    match fs::symlink_metadata(&path) {
        Ok(metadata) => ensure!(
            metadata.is_file(),
            "restore lock must be a regular file: {}",
            path.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect restore lock"),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    let file = options.open(&path).context("open restore lock")?;
    ensure!(
        file.metadata()?.is_file(),
        "restore lock is not a regular file"
    );
    let locked = if exclusive {
        file.try_lock()
    } else {
        file.try_lock_shared()
    };
    match locked {
        Ok(()) => Ok(RestoreGuard { _file: file }),
        Err(TryLockError::WouldBlock) => {
            bail!("another duskcode process is running or restoring data; close it before retrying")
        }
        Err(TryLockError::Error(error)) => {
            Err(error).context("lock duskcode data against offline restore")
        }
    }
}

// Unlike release resources, restore coordination must not move when XDG_DATA_HOME or
// DUSKCODE_RESOURCE_DIR changes while the same HOME/config/workspace roots stay in use.
fn restore_control_root() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME is not set")?;
    let root = PathBuf::from(home).join(".local/share/duskcode");
    ensure!(
        root.is_absolute(),
        "restore control directory must be absolute"
    );
    Ok(root)
}

fn pending_restore_path() -> Result<PathBuf> {
    Ok(restore_control_root()?.join(PENDING_RESTORE))
}

fn require_no_pending_restore() -> Result<()> {
    let pending = pending_restore_path()?;
    match fs::symlink_metadata(&pending) {
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("inspect restore transaction journal"),
        Ok(_) => bail!(
            "an interrupted restore blocks startup and new restores. Run duskcode restore --recover with all other duskcode processes closed. Preserve the private transaction journal at {} and its staging directories; do not delete the journal to bypass recovery",
            pending.display()
        ),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RestoreTransaction {
    format: String,
    version: u32,
    config_root: PathBuf,
    workspace_root: PathBuf,
    config_staging: PathBuf,
    workspace_staging: PathBuf,
}

fn publish_transaction(
    paths: &Paths,
    config: &Path,
    workspace: &Path,
    manifest: &[u8],
) -> Result<()> {
    let root = restore_control_root()?;
    let mut journal = Temporary::new(&root, "restore-journal")?;
    let transaction = RestoreTransaction {
        format: "duskcode-restore-transaction".into(),
        version: VERSION,
        config_root: paths.config_root().to_owned(),
        workspace_root: paths.workspace_root().to_owned(),
        config_staging: config.to_owned(),
        workspace_staging: workspace.to_owned(),
    };
    let json = serde_json::to_vec(&transaction)?;
    ensure!(
        json.len() as u64 <= MAX_TRANSACTION_BYTES,
        "restore transaction paths are too long"
    );
    for (name, contents) in [
        ("transaction.json", json.as_slice()),
        ("manifest.json", manifest),
    ] {
        let mut file = private_file(&journal.path.join(name))?;
        file.write_all(contents)?;
        file.sync_all()?;
    }
    sync_directory(&journal.path)?;
    require_no_pending_restore()?;
    fs::rename(&journal.path, root.join(PENDING_RESTORE))
        .context("publish restore transaction journal")?;
    journal.keep = true;
    // No destination changes are permitted until this durable marker is visible.
    sync_directory(&root).context("sync restore transaction journal before installing data")
}

/// Recovery is explicit. It never trusts a phase flag: after a crash, each root must
/// exist either in its recorded staging location or at its destination, and must match
/// every recorded file/directory/hash before recovery changes either destination.
pub fn recover(paths: &Paths) -> Result<()> {
    let _guard = restore_guard(true)?;
    let pending = pending_restore_path()?;
    inspect_tree_path(&pending)?;
    ensure!(
        fs::symlink_metadata(&pending)
            .context("no pending restore journal; there is nothing to recover")?
            .is_dir(),
        "restore journal must be a real directory"
    );
    let transaction: RestoreTransaction = serde_json::from_slice(&read_bounded(
        &pending.join("transaction.json"),
        MAX_TRANSACTION_BYTES,
    )?)
    .context(
        "invalid restore transaction; preserve the journal and staging data for manual recovery",
    )?;
    ensure!(
        transaction.format == "duskcode-restore-transaction" && transaction.version == VERSION,
        "unsupported restore transaction format"
    );
    ensure!(
        transaction.config_root == paths.config_root()
            && transaction.workspace_root == paths.workspace_root(),
        "restore journal belongs to different data roots; preserve it and recover under the original HOME"
    );
    validate_staging_path(&transaction.config_staging, paths.config_root(), "config")?;
    validate_staging_path(
        &transaction.workspace_staging,
        paths.workspace_root(),
        "workspace",
    )?;
    let manifest: Manifest = serde_json::from_slice(&read_bounded(
        &pending.join("manifest.json"),
        MAX_MANIFEST_BYTES,
    )?)
    .context("invalid journaled archive manifest")?;
    validate_manifest(&manifest)?;
    let mut config = Temporary {
        path: transaction.config_staging,
        keep: true,
        offline: true,
    };
    let mut workspace = Temporary {
        path: transaction.workspace_staging,
        keep: true,
        offline: true,
    };
    finish_install(paths, &manifest, &mut config, &mut workspace)
}

fn validate_staging_path(staging: &Path, destination: &Path, name: &str) -> Result<()> {
    ensure!(
        staging.parent() == destination.parent(),
        "restore staging directory has an unexpected parent"
    );
    let filename = staging
        .file_name()
        .and_then(|name| name.to_str())
        .context("invalid restore staging filename")?;
    let prefix = format!(".duskcode-restore-{name}-");
    let id = filename
        .strip_prefix(&prefix)
        .context("invalid restore staging directory name")?;
    Uuid::parse_str(id).context("invalid restore staging directory ID")?;
    inspect_tree_path(staging)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RootState {
    Staged,
    Installed,
}

fn root_state(
    staging: &Path,
    destination: &Path,
    name: &str,
    manifest: &Manifest,
) -> Result<RootState> {
    match fs::symlink_metadata(staging) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "restore staging container must be a real directory"
            );
            for entry in fs::read_dir(staging)? {
                ensure!(
                    entry?.file_name() == "root",
                    "restore staging contains unexpected data; preserve it for manual recovery"
                );
            }
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect restore staging container"),
    }
    let staged_root = staging.join("root");
    match fs::symlink_metadata(&staged_root) {
        Ok(_) => {
            verify_tree(&staged_root, name, manifest)?;
            preflight(destination)?;
            Ok(RootState::Staged)
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let pending = pending_restore_path()?;
            verify_tree(destination, name, manifest).with_context(|| format!("installed {name} data does not match the restore journal; preserve both roots, {} and all staging directories for manual recovery", pending.display()))?;
            Ok(RootState::Installed)
        }
        Err(error) => Err(error).context("inspect journaled restore staging"),
    }
}

fn finish_install(
    paths: &Paths,
    manifest: &Manifest,
    config: &mut Temporary,
    workspace: &mut Temporary,
) -> Result<()> {
    let config_state = root_state(&config.path, paths.config_root(), "config", manifest)?;
    let workspace_state = root_state(
        &workspace.path,
        paths.workspace_root(),
        "workspace",
        manifest,
    )?;
    let installed = (|| -> Result<()> {
        install_root(&config.path, paths.config_root(), config_state)?;
        install_root(&workspace.path, paths.workspace_root(), workspace_state)
    })();
    if let Err(error) = installed {
        // Roll back roots newly published by this attempt, in reverse order. Never
        // delete data; even an unexpected concurrent writer's additions remain staged.
        let workspace_rollback =
            rollback_root(&workspace.path, paths.workspace_root(), workspace_state);
        let config_rollback = rollback_root(&config.path, paths.config_root(), config_state);
        let recovery = pending_restore_path()?;
        if workspace_rollback.is_err() || config_rollback.is_err() {
            bail!(
                "restore installation failed: {error:#}; rollback results: workspace={}, config={}. Startup remains blocked. Preserve journal {} and staging {} and {}; resolve the destination conflict, then run duskcode restore --recover",
                workspace_rollback
                    .err()
                    .map_or_else(|| "ok".to_owned(), |error| format!("{error:#}")),
                config_rollback
                    .err()
                    .map_or_else(|| "ok".to_owned(), |error| format!("{error:#}")),
                recovery.display(),
                config.path.display(),
                workspace.path.display()
            );
        }
        bail!(
            "restore installation failed: {error:#}. Startup remains blocked. Preserve journal {} and staging {} and {}; resolve the destination conflict, then run duskcode restore --recover",
            recovery.display(),
            config.path.display(),
            workspace.path.display()
        );
    }
    // Both renames (including a prior interrupted attempt's renames) must be durable
    // before retiring the marker. A crash before retirement still requires recovery.
    sync_directory(
        paths
            .config_root()
            .parent()
            .context("config parent is missing")?,
    )?;
    sync_directory(
        paths
            .workspace_root()
            .parent()
            .context("workspace parent is missing")?,
    )?;
    retire_transaction()?;
    config.keep = false;
    workspace.keep = false;
    Ok(())
}

fn rollback_root(staging: &Path, destination: &Path, original: RootState) -> Result<()> {
    if original != RootState::Staged {
        return Ok(());
    }
    let root = staging.join("root");
    match fs::symlink_metadata(&root) {
        Ok(_) => return Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect root before restore rollback"),
    }
    fs::rename(destination, &root).context("move installed root back into retained staging")?;
    sync_directory(staging)?;
    sync_directory(
        destination
            .parent()
            .context("rollback destination parent is missing")?,
    )
}

fn install_root(staging: &Path, destination: &Path, state: RootState) -> Result<()> {
    if state == RootState::Staged {
        remove_empty_destination(destination)?;
        fs::rename(staging.join("root"), destination).context("publish restored data root")?;
        sync_directory(staging)?;
    }
    sync_directory(
        destination
            .parent()
            .context("restore destination has no parent")?,
    )
}

fn retire_transaction() -> Result<()> {
    let root = restore_control_root()?;
    let retired = root.join(format!(".restore-completed-{}", Uuid::now_v7()));
    fs::rename(root.join(PENDING_RESTORE), &retired)
        .context("retire completed restore transaction")?;
    sync_directory(&root).with_context(|| {
        format!(
            "both roots are installed, but final journal sync failed; preserve {}",
            retired.display()
        )
    })?;
    if let Err(error) = fs::remove_dir_all(&retired) {
        eprintln!(
            "duskcode: restore completed; private completed journal remains at {}: {error}",
            retired.display()
        );
    }
    Ok(())
}

fn sync_staging(staging: &Path, name: &str, manifest: &Manifest) -> Result<()> {
    for entry in manifest.entries.iter().rev().filter(|entry| {
        entry.kind == Kind::Directory
            && (entry.path == name || entry.path.starts_with(&format!("{name}/")))
    }) {
        let relative = entry
            .path
            .strip_prefix(name)
            .unwrap_or("")
            .trim_start_matches('/');
        sync_directory(&staging.join("root").join(relative))?;
    }
    sync_directory(staging)?;
    sync_directory(
        staging
            .parent()
            .context("staging directory has no parent")?,
    )
}

fn sync_directory(directory: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(directory)?
            .sync_all()
            .with_context(|| format!("sync application directory {}", directory.display()))?;
    }
    // std does not expose a portable directory flush on other platforms. File syncs
    // and atomic rename still apply; full power-loss durability depends on the OS/FS.
    #[cfg(not(unix))]
    {
        let _ = directory;
    }
    Ok(())
}

fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    let mut file = open_regular_input(path)?;
    ensure!(
        file.metadata()?.len() <= maximum,
        "restore journal file is too large"
    );
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= maximum,
        "restore journal file is too large"
    );
    Ok(bytes)
}

fn open_regular_input(path: &Path) -> Result<File> {
    let before =
        fs::symlink_metadata(path).context("inspect archive or journal input before opening")?;
    ensure!(
        before.is_file(),
        "archive or journal input must be a regular file, not a symlink or special file"
    );
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // A FIFO substituted after the metadata check cannot block open, and a final
        // symlink substituted in that window cannot redirect it to a different target.
        options.custom_flags(nix::libc::O_NONBLOCK | nix::libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .context("open regular archive or journal input")?;
    ensure_same_file(&before, &file.metadata()?)?;
    Ok(file)
}

fn verify_tree(root: &Path, name: &str, manifest: &Manifest) -> Result<()> {
    inspect_tree_path(root)?;
    let prefix = format!("{name}/");
    let expected: BTreeMap<_, _> = manifest
        .entries
        .iter()
        .filter(|entry| entry.path == name || entry.path.starts_with(&prefix))
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut seen = 0usize;
    verify_tree_entry(root, name, &expected, &mut seen)?;
    ensure!(
        seen == expected.len(),
        "restored tree is missing journaled entries"
    );
    Ok(())
}

fn verify_tree_entry(
    path: &Path,
    name: &str,
    expected: &BTreeMap<&str, &Entry>,
    seen: &mut usize,
) -> Result<()> {
    validate_path(name)?;
    let entry = expected
        .get(name)
        .context("restored tree contains data absent from the journal; nothing was overwritten")?;
    let metadata = fs::symlink_metadata(path)?;
    *seen += 1;
    match entry.kind {
        Kind::Directory => {
            ensure!(
                metadata.is_dir(),
                "journaled directory is missing or changed"
            );
            for child in fs::read_dir(path)? {
                let child = child?;
                let filename = child
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("restored tree contains a non-UTF-8 filename"))?;
                verify_tree_entry(&child.path(), &format!("{name}/{filename}"), expected, seen)?;
            }
        }
        Kind::File => {
            ensure!(
                metadata.is_file() && metadata.len() == entry.size,
                "journaled file is missing or changed"
            );
            #[cfg(unix)]
            ensure!(
                executable(&metadata) == entry.executable,
                "journaled file executable permission changed"
            );
            let mut file = open_regular_input(path)?;
            let (size, hash) = copy_hash(
                &mut Read::by_ref(&mut file).take(entry.size + 1),
                &mut io::sink(),
            )?;
            ensure!(
                size == entry.size && Some(&hash) == entry.sha256.as_ref(),
                "journaled file checksum mismatch"
            );
        }
    }
    Ok(())
}

fn remove_empty_destination(path: &Path) -> Result<bool> {
    preflight(path)?;
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("destination is no longer empty: {}", path.display())),
    }
}

fn preflight(path: &Path) -> Result<()> {
    inspect_tree_path(path)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_dir(),
                "restore destination must be a real directory: {}",
                path.display()
            );
            ensure!(
                fs::read_dir(path)?.next().transpose()?.is_none(),
                "restore refuses non-empty destination {}; move its data elsewhere before retrying (archives must also be outside both destination roots)",
                path.display()
            );
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("inspect restore destination"),
    }
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<u64> {
    ensure!(
        manifest.format == FORMAT && manifest.version == VERSION,
        "unsupported duskcode archive format or version"
    );
    ensure!(
        manifest.excluded_workspace_paths == vec![BACKUPS.to_owned()],
        "unsupported archive exclusions"
    );
    ensure!(
        !manifest.entries.is_empty() && manifest.entries.len() <= MAX_ENTRIES,
        "invalid archive entry count"
    );
    let mut seen = BTreeMap::new();
    let mut bytes = 0u64;
    for entry in &manifest.entries {
        validate_path(&entry.path)?;
        ensure!(
            !seen.contains_key(entry.path.as_str()),
            "duplicate archive path"
        );
        if let Some((parent, _)) = entry.path.rsplit_once('/') {
            ensure!(
                seen.get(parent) == Some(&Kind::Directory),
                "archive entry has no earlier directory parent"
            );
        }
        ensure!(
            entry.path != "workspace/backups" && !entry.path.starts_with("workspace/backups/"),
            "backup directory must not be inside the archive payload"
        );
        match entry.kind {
            Kind::Directory => ensure!(
                entry.size == 0 && entry.sha256.is_none() && !entry.executable,
                "invalid directory entry"
            ),
            Kind::File => {
                ensure!(entry.path.contains('/'), "archive root must be a directory");
                ensure!(
                    entry.sha256.as_ref().is_some_and(|hash| hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
                    "invalid archive checksum"
                );
                bytes = bytes
                    .checked_add(entry.size)
                    .context("archive payload size overflow")?;
                ensure!(
                    bytes <= MAX_PAYLOAD_BYTES,
                    "archive payload exceeds the 1 TiB limit"
                );
            }
        }
        seen.insert(entry.path.as_str(), entry.kind);
    }
    ensure!(
        seen.get("config") == Some(&Kind::Directory)
            && seen.get("workspace") == Some(&Kind::Directory),
        "archive must contain both data roots"
    );
    ensure!(
        seen.get("config/session/state.db") == Some(&Kind::File),
        "archive is missing the SQLite snapshot"
    );
    for suffix in ["-wal", "-shm", "-journal"] {
        ensure!(
            !seen.contains_key(format!("config/session/state.db{suffix}").as_str()),
            "archive contains an active database sidecar"
        );
    }
    Ok(bytes)
}

fn validate_path(path: &str) -> Result<()> {
    ensure!(
        path.len() <= MAX_PATH_BYTES,
        "archive path exceeds the 4096-byte limit"
    );
    let parts: Vec<_> = path.split('/').collect();
    ensure!(
        parts.len() <= MAX_DEPTH && matches!(parts[0], "config" | "workspace"),
        "invalid archive root or path depth"
    );
    for part in parts {
        validate_segment(part)?;
    }
    Ok(())
}

fn validate_segment(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && name.len() <= 255
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| c.is_control()
                || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')),
        "archive contains a non-portable filename or unsafe path"
    );
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(device.len() == 4
                && (device.starts_with("COM") || device.starts_with("LPT"))
                && matches!(device.as_bytes()[3], b'1'..=b'9')),
        "archive contains a reserved device filename"
    );
    Ok(())
}

/// Inspect every existing component, including ancestors, without following links. Missing
/// suffixes are allowed; callers create them one directory at a time, never recursively.
fn inspect_tree_path(path: &Path) -> Result<()> {
    ensure!(path.is_absolute(), "data path must be absolute");
    let mut current = PathBuf::new();
    for component in path.components() {
        ensure!(
            !matches!(component, Component::ParentDir | Component::CurDir),
            "data path must not contain . or .."
        );
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                !metadata.file_type().is_symlink(),
                "data path must not contain a symlink: {}",
                current.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("inspect {}", current.display()));
            }
        }
    }
    Ok(())
}

fn directory_tree(path: &Path) -> Result<()> {
    inspect_tree_path(path)?;
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir(),
                "expected a real directory: {}",
                current.display()
            ),
            Err(error) if error.kind() == ErrorKind::NotFound => {
                private_dir(&current)?;
                sync_directory(current.parent().context("new directory has no parent")?)?;
            }
            Err(error) => return Err(error).context("inspect directory"),
        }
    }
    Ok(())
}

fn private_dir(path: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(path)
        .with_context(|| format!("create private directory {}", path.display()))
}

fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("create private file {}", path.display()))
}

fn executable(metadata: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        false
    }
}

fn set_executable(file: &File, executable: bool) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(if executable {
            0o700
        } else {
            0o600
        }))?;
    }
    #[cfg(not(unix))]
    {
        let _ = (file, executable);
    }
    Ok(())
}

fn ensure_same_file(before: &Metadata, opened: &Metadata) -> Result<()> {
    ensure!(opened.is_file(), "archive source changed to a special file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == opened.dev() && before.ino() == opened.ino(),
            "archive source changed while opening it"
        );
    }
    ensure!(
        before.len() == opened.len(),
        "archive source changed while opening it"
    );
    Ok(())
}

fn copy_hash(input: &mut impl Read, output: &mut impl Write) -> Result<(u64, String)> {
    let mut buffer = [0; BUFFER_BYTES];
    let mut hash = Sha256::new();
    let mut copied = 0u64;
    loop {
        let bytes = input.read(&mut buffer)?;
        if bytes == 0 {
            break;
        }
        output.write_all(&buffer[..bytes])?;
        hash.update(&buffer[..bytes]);
        copied = copied
            .checked_add(bytes as u64)
            .context("file size overflow")?;
    }
    let hash = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    Ok((copied, hash))
}

struct Temporary {
    path: PathBuf,
    keep: bool,
    offline: bool,
}

impl Temporary {
    fn new(parent: &Path, purpose: &str) -> Result<Self> {
        let path = parent.join(format!(".duskcode-{purpose}-{}", Uuid::now_v7()));
        private_dir(&path)?;
        Ok(Self {
            path,
            keep: false,
            offline: purpose.starts_with("restore"),
        })
    }
}

impl Drop for Temporary {
    fn drop(&mut self) {
        if !self.keep {
            if let Err(error) = fs::remove_dir_all(&self.path) {
                if error.kind() == ErrorKind::NotFound {
                    return;
                }
                tracing::warn!(
                    "cannot remove private archive staging directory {}: {error}",
                    self.path.display()
                );
                if self.offline {
                    eprintln!(
                        "duskcode: private restore staging remains at {}: {error}",
                        self.path.display()
                    );
                }
            }
        }
    }
}
