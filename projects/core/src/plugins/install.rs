//! A plugin's skills and agents, put where the core reads its own: `skills/<plugin>/`
//! and `agents/<plugin>/`. A record of what was written, by hash, tells a file the
//! plugin put there from one the user edited since: an edited file is never replaced or
//! removed.

use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};

use crate::{init::paths::Paths, release::resources};

/// Far above any skill or agent file.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const PARTS: [&str; 2] = ["skills", "agents"];

/// Brings the installed copy of `plugin`'s skills and agents to what `folder` holds; with
/// `folder` `None` the plugin is off and its untouched files go.
pub fn sync(paths: &Paths, plugin: &str, folder: Option<&Path>) -> Result<()> {
    let record_file = record_file(paths, plugin);
    let mut record = read_record(&record_file);
    let mut wanted = BTreeMap::new();
    if let Some(folder) = folder {
        for part in PARTS {
            let source = folder.join(part);
            if !source.is_dir() {
                continue;
            }
            for relative in resources::files(&source)? {
                let bytes =
                    crate::init::files::read_regular(&source.join(&relative), MAX_FILE_BYTES)?;
                wanted.insert(Path::new(part).join(plugin).join(relative), bytes);
            }
        }
    }
    let root = paths.rules_root();
    for (relative, bytes) in &wanted {
        let target = root.join(relative);
        let on_disk = fs::read(&target).ok().map(|disk| hash(&disk));
        let ours = record.get(relative);
        // Missing, or still as the plugin wrote it: the plugin's version goes in.
        if on_disk.is_none() || on_disk.as_ref() == ours {
            crate::init::create_private_dir(target.parent().context("no folder")?)?;
            crate::init::files::write_private(&target, bytes)?;
            record.insert(relative.clone(), hash(bytes));
        }
    }
    let gone: Vec<PathBuf> = record
        .keys()
        .filter(|relative| !wanted.contains_key(*relative))
        .cloned()
        .collect();
    for relative in gone {
        let target = root.join(&relative);
        if fs::read(&target).ok().map(|disk| hash(&disk)).as_ref() == record.get(&relative) {
            let _ = fs::remove_file(&target);
        }
        record.remove(&relative);
    }
    write_record(&record_file, &record)
}

/// Beside the plugins, so it goes when the user removes the folder of plugins whole.
fn record_file(paths: &Paths, plugin: &str) -> PathBuf {
    super::catalog::folder(paths).join(format!(".installed-{plugin}"))
}

/// Lines of `hash path`; anything else is dropped, which only means a file is kept.
fn read_record(file: &Path) -> BTreeMap<PathBuf, String> {
    let text = fs::read_to_string(file).unwrap_or_default();
    text.lines()
        .filter_map(|line| line.split_once(' '))
        .map(|(hash, path)| (PathBuf::from(path), hash.to_owned()))
        .filter(|(path, _)| {
            path.components()
                .all(|part| matches!(part, Component::Normal(_)))
                && PARTS.iter().any(|part| path.starts_with(part))
        })
        .collect()
}

fn write_record(file: &Path, record: &BTreeMap<PathBuf, String>) -> Result<()> {
    if record.is_empty() {
        let _ = fs::remove_file(file);
        return Ok(());
    }
    let mut text = String::new();
    for (path, hash) in record {
        let path = path.to_str().context("a plugin file name is not UTF-8")?;
        ensure!(
            !path.contains('\n'),
            "a plugin file name holds a line break"
        );
        text.push_str(&format!("{hash} {path}\n"));
    }
    crate::init::files::write_private(file, text.as_bytes())
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
