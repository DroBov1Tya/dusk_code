//! A plugin's skills and agents, put where the core reads its own: `skills/<plugin>/`
//! and `agents/<plugin>/`. A record of what was written, by hash, tells a file the
//! plugin put there from one the user edited since: an edited file is never replaced or
//! removed. The store places skill and agent packs the same way, under its own record.

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
    write_wanted(paths, &record_file, &wanted)
}

/// Brings the files under the rules folder to `wanted`, by path there, with `record_file`
/// telling what was written before: a file the user edited since is kept, written or not,
/// and the folders that removal empties go.
pub fn write_wanted(
    paths: &Paths,
    record_file: &Path,
    wanted: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    let before = read_record(record_file);
    let mut record = before.clone();
    let root = paths.rules_root();
    for (relative, bytes) in wanted {
        let target = root.join(relative);
        let on_disk = fs::read(&target).ok().map(|disk| hash(&disk));
        let ours = record.get(relative);
        // Missing, or still as the plugin wrote it: the plugin's version goes in, unless
        // the file already holds it, as on every scan when nothing changed.
        if on_disk.is_none() || on_disk.as_ref() == ours {
            let new = hash(bytes);
            if on_disk.as_ref() != Some(&new) {
                crate::init::create_private_dir(target.parent().context("no folder")?)?;
                crate::init::files::write_private(&target, bytes)?;
            }
            record.insert(relative.clone(), new);
        }
    }
    let gone: Vec<PathBuf> = record
        .keys()
        .filter(|relative| !wanted.contains_key(*relative))
        .cloned()
        .collect();
    for relative in gone {
        let target = root.join(&relative);
        if fs::read(&target).ok().map(|disk| hash(&disk)).as_ref() == record.get(&relative)
            && fs::remove_file(&target).is_ok()
        {
            remove_empty_parents(&root, &target);
        }
        record.remove(&relative);
    }
    if record == before {
        return Ok(());
    }
    write_record(record_file, &record)
}

/// Folders left empty under `skills/` or `agents/`; one that holds anything stays.
fn remove_empty_parents(root: &Path, file: &Path) {
    let mut folder = file.parent();
    while let Some(parent) = folder {
        let part = parent.strip_prefix(root).ok();
        if part.is_none_or(|part| part.components().count() < 2) || fs::remove_dir(parent).is_err()
        {
            break;
        }
        folder = parent.parent();
    }
}

/// Beside the plugins, so it goes when the user removes the folder of plugins whole.
fn record_file(paths: &Paths, plugin: &str) -> PathBuf {
    super::catalog::folder(paths).join(format!(".installed-{plugin}"))
}

/// Lines of `hash path`; anything else is dropped, which only means a file is kept.
pub(crate) fn read_record(file: &Path) -> BTreeMap<PathBuf, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_already_as_wanted_is_not_written_again() {
        let paths = crate::testing::temp_paths("install-unchanged");
        let record = paths.rules_root().join(".installed-test");
        let relative = PathBuf::from("skills/test/one/SKILL.md");
        let wanted = BTreeMap::from([(relative.clone(), b"first".to_vec())]);
        write_wanted(&paths, &record, &wanted).unwrap();
        let target = paths.rules_root().join(&relative);
        assert_eq!(fs::read(&target).unwrap(), b"first");
        // A stamp far in the past: a rewrite would move it to now.
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        for file in [&target, &record] {
            fs::File::options()
                .write(true)
                .open(file)
                .unwrap()
                .set_modified(old)
                .unwrap();
        }
        write_wanted(&paths, &record, &wanted).unwrap();
        for file in [&target, &record] {
            assert_eq!(fs::metadata(file).unwrap().modified().unwrap(), old);
        }
        // A new version from the plugin still goes in, and one the user edited stays.
        let newer = BTreeMap::from([(relative.clone(), b"second".to_vec())]);
        write_wanted(&paths, &record, &newer).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second");
        fs::write(&target, b"edited").unwrap();
        write_wanted(&paths, &record, &wanted).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"edited");
    }
}
