//! Private portable archives. Version 1 is an uncompressed stream: magic, a little-endian
//! u64 JSON-manifest length, the manifest, then regular-file contents in manifest order.
//! No executable archive parser or external archiver is involved. Links and special files
//! are refused rather than followed; saved paths inside file contents are not rewritten.

use std::{
    collections::BTreeMap,
    fs::{self, DirBuilder, File, Metadata, OpenOptions},
    io::{self, BufReader, BufWriter, ErrorKind, Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    let paths = paths.clone();
    tokio::task::spawn_blocking(move || export_files(&paths, &snapshot, &destination, temporary))
        .await
        .context("archive writer stopped")?
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
    drop(output);
    // Unlike rename, creating a hard link never replaces a concurrently created output.
    // Staging and output are both inside backups, so they are on the same filesystem.
    fs::hard_link(&archive_path, destination)
        .context("publish the archive without overwriting an existing file")?;
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
    let mut input = File::open(source)?;
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
    preflight(paths.config_root())?;
    preflight(paths.workspace_root())?;
    ensure!(
        !paths.config_root().starts_with(paths.workspace_root())
            && !paths.workspace_root().starts_with(paths.config_root()),
        "restore roots must be separate directories"
    );
    let mut input = BufReader::new(File::open(archive).context("open migration archive")?);
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
            output.sync_all()?;
            set_executable(&output, entry.executable)?;
        }
    }
    let mut end = [0];
    ensure!(input.read(&mut end)? == 0, "archive has trailing data");
    preflight(paths.config_root())?;
    preflight(paths.workspace_root())?;
    install(paths, &mut config, &mut workspace)
}

fn install(paths: &Paths, config: &mut Temporary, workspace: &mut Temporary) -> Result<()> {
    let config_staged = config.path.join("root");
    let workspace_staged = workspace.path.join("root");
    // Remove only genuinely empty directories. A new writer makes remove_dir fail;
    // rename also refuses a non-empty directory and never follows a target symlink.
    let config_existed = remove_empty_destination(paths.config_root())?;
    let workspace_existed = match remove_empty_destination(paths.workspace_root()) {
        Ok(existed) => existed,
        Err(error) => {
            if config_existed {
                let _ = private_dir(paths.config_root());
            }
            return Err(error);
        }
    };
    if let Err(error) = fs::rename(&config_staged, paths.config_root()) {
        if config_existed {
            let _ = private_dir(paths.config_root());
        }
        if workspace_existed {
            let _ = private_dir(paths.workspace_root());
        }
        return Err(error).context("install restored config; existing data was not replaced");
    }
    if let Err(error) = fs::rename(&workspace_staged, paths.workspace_root()) {
        // Retain recovery data if rollback fails. Never delete a destination containing
        // restored or concurrently-created user data in an attempt to repair a failure.
        if let Err(rollback) = fs::rename(paths.config_root(), &config_staged) {
            config.keep = true;
            workspace.keep = true;
            bail!(
                "restore stopped after config installation: {error}; rollback failed: {rollback}. Config remains at {}. Recovery data is retained at {} and {}; preserve these paths and resolve the destination conflict before retrying",
                paths.config_root().display(),
                config.path.display(),
                workspace.path.display()
            );
        }
        if config_existed {
            let _ = private_dir(paths.config_root());
        }
        if workspace_existed {
            let _ = private_dir(paths.workspace_root());
        }
        // Even successful rollback may have moved data written by another process.
        // Retain that config tree rather than deleting potentially new user data.
        config.keep = true;
        bail!(
            "restore was rolled back because workspace installation failed: {error}. Config recovery data is retained at {}; no destination data was overwritten. Close other duskcode processes and resolve the conflict before retrying",
            config.path.display()
        );
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
            Err(error) if error.kind() == ErrorKind::NotFound => private_dir(&current)?,
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
