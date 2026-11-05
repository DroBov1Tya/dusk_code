//! Checks of a staged tree before anything is installed.

use std::{
    collections::BTreeMap,
    fs::{self},
    io::{self, ErrorKind, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{
    files::{copy_hash, executable, open_regular_input, private_dir, sync_directory},
    manifest::{Entry, Kind, Manifest, validate_path},
};

pub fn verify_tree(root: &Path, name: &str, manifest: &Manifest) -> Result<()> {
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

pub fn verify_tree_entry(
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

/// Inspect every existing component, including ancestors, without following links. Missing
/// suffixes are allowed; callers create them one directory at a time, never recursively.
pub fn inspect_tree_path(path: &Path) -> Result<()> {
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

pub fn directory_tree(path: &Path) -> Result<()> {
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
