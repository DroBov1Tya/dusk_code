//! What an export takes: the files under the data roots, counted against the limits.

use std::{
    fs::{self},
    io::Read,
    path::Path,
};

use anyhow::{Context, Result, ensure};

use crate::init::paths::Paths;

use super::{
    BACKUPS, MAX_ENTRIES, MAX_MANIFEST_BYTES, MAX_PAYLOAD_BYTES,
    files::{
        copy_hash, ensure_same_file, executable, open_regular_input, private_dir, private_file,
    },
    manifest::{Entry, Kind, validate_path},
    verify::inspect_tree_path,
};

pub fn collect(
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

pub fn collect_file(
    source: &Path,
    target: &Path,
    name: &str,
    inventory: &mut Inventory,
) -> Result<()> {
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
pub struct Inventory {
    pub entries: Vec<Entry>,
    pub payload_bytes: u64,
    pub entry_json_bytes: u64,
}

impl Inventory {
    /// Reserve bounds before copying another source file. Account for the JSON representation
    /// of each entry, plus room for the fixed manifest fields, so inventory RAM stays bounded.
    pub fn reserve(&mut self, entry: &Entry) -> Result<()> {
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

pub fn database_sidecar(path: &Path, database: &Path) -> bool {
    ["-wal", "-shm", "-journal"].iter().any(|suffix| {
        let mut name = database.as_os_str().to_owned();
        name.push(suffix);
        path == Path::new(&name)
    })
}
