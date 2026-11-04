//! Checks of a manifest and of a staged tree before anything is installed.

use std::{
    collections::BTreeMap,
    fs::{self},
    io::{self, ErrorKind, Read},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::{
    BACKUPS, Entry, FORMAT, Kind, MAX_DEPTH, MAX_ENTRIES, MAX_PATH_BYTES, MAX_PAYLOAD_BYTES,
    Manifest, VERSION,
    files::{copy_hash, executable, open_regular_input, private_dir, sync_directory},
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

pub fn validate_manifest(manifest: &Manifest) -> Result<u64> {
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

pub fn validate_path(path: &str) -> Result<()> {
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

pub fn validate_segment(name: &str) -> Result<()> {
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
