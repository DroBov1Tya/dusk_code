//! Restoring an archive offline: stages it, verifies it and swaps the roots in a recoverable transaction.

use std::{
    fs::{self},
    io::{BufReader, ErrorKind, Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::init::paths::Paths;

use super::{
    Kind, MAGIC, MAX_MANIFEST_BYTES, MAX_TRANSACTION_BYTES, Manifest, PENDING_RESTORE, VERSION,
    files::{
        Temporary, copy_hash, open_regular_input, private_dir, private_file, read_bounded,
        set_executable, sync_directory,
    },
    guard::{
        pending_restore_path, require_no_pending_restore, restore_control_root, restore_guard,
    },
    verify::{directory_tree, inspect_tree_path, validate_manifest, verify_tree},
};

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

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreTransaction {
    pub format: String,
    pub version: u32,
    pub config_root: PathBuf,
    pub workspace_root: PathBuf,
    pub config_staging: PathBuf,
    pub workspace_staging: PathBuf,
}

pub fn publish_transaction(
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

pub fn validate_staging_path(staging: &Path, destination: &Path, name: &str) -> Result<()> {
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
pub enum RootState {
    Staged,
    Installed,
}

pub fn root_state(
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

pub fn finish_install(
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

pub fn rollback_root(staging: &Path, destination: &Path, original: RootState) -> Result<()> {
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

pub fn install_root(staging: &Path, destination: &Path, state: RootState) -> Result<()> {
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

pub fn retire_transaction() -> Result<()> {
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

pub fn sync_staging(staging: &Path, name: &str, manifest: &Manifest) -> Result<()> {
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

pub fn remove_empty_destination(path: &Path) -> Result<bool> {
    preflight(path)?;
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error)
            .with_context(|| format!("destination is no longer empty: {}", path.display())),
    }
}

pub fn preflight(path: &Path) -> Result<()> {
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
