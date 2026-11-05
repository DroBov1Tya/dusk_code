//! The recoverable swap of the restored roots into place, and its recovery after a crash.

use std::{
    fs::{self},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::init::paths::Paths;

use super::{
    MAX_MANIFEST_BYTES, MAX_TRANSACTION_BYTES, PENDING_RESTORE, VERSION,
    files::{Temporary, private_file, read_bounded, sync_directory},
    manifest::{Manifest, validate_manifest},
    restore::{preflight, remove_empty_destination},
    restore_guard::{
        pending_restore_path, require_no_pending_restore, restore_control_root, restore_guard,
    },
    verify::{inspect_tree_path, verify_tree},
};

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
