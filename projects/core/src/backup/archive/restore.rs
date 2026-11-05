//! Restoring an archive offline: stages it and verifies it before anything is installed.

use std::{
    fs::{self},
    io::{BufReader, ErrorKind, Read},
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::init::paths::Paths;

use super::{
    MAGIC, MAX_MANIFEST_BYTES,
    files::{
        Temporary, copy_hash, open_regular_input, private_dir, private_file, set_executable,
        sync_directory,
    },
    manifest::{Kind, Manifest, validate_manifest},
    restore_guard::{pending_restore_path, require_no_pending_restore, restore_guard},
    transaction::{finish_install, publish_transaction},
    verify::{directory_tree, inspect_tree_path},
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
