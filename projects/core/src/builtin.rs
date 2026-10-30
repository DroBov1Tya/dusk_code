//! The files dusk ships into the config folder: the prompts of the modes, COMPACT.md,
//! FORK.md and the built-in skills. At every start each is brought to the version of
//! this build, without losing what the user wrote: `builtin.lock` records the hash of
//! each file as dusk last wrote it. A file still as written is replaced; a file the
//! user changed, or one from before the record, is kept, and the new version is put
//! beside it as `<file>.new`.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use include_dir::{Dir, DirEntry};
use sha2::{Digest, Sha256};

use crate::{
    init::{COMPACT_PROMPT_TEMPLATE, FORK_PROMPT_TEMPLATE, create_private_dir},
    modes,
    paths::Paths,
    skills::BUILTIN,
};

const LOCK_FILE: &str = "builtin.lock";
const NEW_SUFFIX: &str = ".new";

/// What a start did to the shipped files, by their path in the config folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    pub written: Vec<String>,
    pub updated: Vec<String>,
    /// Changed by the user and kept; the new version is beside each as `.new`. Only
    /// files whose `.new` is new or changed this start, so each is told once.
    pub kept: Vec<String>,
}

pub fn sync(paths: &Paths) -> Result<Synced> {
    let root = paths.config_root();
    let lock_path = root.join(LOCK_FILE);
    let mut lock = read_lock(&lock_path);
    let mut synced = Synced::default();
    for (path, contents) in shipped() {
        let target = root.join(&path);
        let shipped_hash = hash(&contents);
        let on_disk = match fs::symlink_metadata(&target) {
            // A link was put there by hand; dusk does not write through it.
            Ok(metadata) if !metadata.is_file() => continue,
            Ok(_) => Some(hash(
                &fs::read(&target).context("cannot read a shipped file")?,
            )),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        match on_disk {
            None => {
                write(&target, &contents)?;
                synced.written.push(path.clone());
            }
            Some(disk) if disk == shipped_hash => {}
            Some(disk) if lock.get(&path) == Some(&disk) => {
                write(&target, &contents)?;
                synced.updated.push(path.clone());
            }
            Some(_) => {
                let beside = PathBuf::from(format!("{}{NEW_SUFFIX}", target.display()));
                if fs::read(&beside).ok().as_deref() != Some(contents.as_slice()) {
                    write(&beside, &contents)?;
                    synced.kept.push(path);
                }
                continue;
            }
        }
        lock.insert(path, shipped_hash);
    }
    write(&lock_path, serde_json::to_string_pretty(&lock)?.as_bytes())?;
    Ok(synced)
}

/// Every shipped file by its path in the config folder, with its contents.
fn shipped() -> Vec<(String, Vec<u8>)> {
    let mut files = vec![
        (
            "COMPACT.md".to_owned(),
            COMPACT_PROMPT_TEMPLATE.as_bytes().to_vec(),
        ),
        (
            "FORK.md".to_owned(),
            FORK_PROMPT_TEMPLATE.as_bytes().to_vec(),
        ),
    ];
    for mode in [modes::MAIN, modes::PLAN, modes::ORCHESTRATOR] {
        let template = modes::template(mode).unwrap_or_default();
        files.push((format!("modes/{mode}.md"), template.as_bytes().to_vec()));
    }
    skill_files(&BUILTIN, &mut files);
    files
}

fn skill_files(dir: &Dir, files: &mut Vec<(String, Vec<u8>)>) {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(dir) => skill_files(dir, files),
            DirEntry::File(file) => {
                let path = format!("skills/{}", file.path().display());
                files.push((path, file.contents().to_vec()));
            }
        }
    }
}

fn read_lock(path: &Path) -> BTreeMap<String, String> {
    fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Written beside the target and moved over it, private to the user. The archive keeps
/// no file modes, so a script says it is one by its first line.
fn write(target: &Path, contents: &[u8]) -> Result<()> {
    if let Some(parent) = target.parent() {
        create_private_dir(parent)?;
    }
    let script = contents.starts_with(b"#!");
    let temporary = PathBuf::from(format!("{}.writing", target.display()));
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(if script { 0o700 } else { 0o600 });
    }
    let mut file = options
        .open(&temporary)
        .with_context(|| format!("cannot write {}", target.display()))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .with_context(|| format!("cannot write {}", target.display()))?;
    fs::rename(&temporary, target).with_context(|| format!("cannot replace {}", target.display()))
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
    use crate::{config::Config, skills, testing::temp_paths};

    #[test]
    fn shipped_files_follow_the_build_unless_the_user_changed_them() {
        let paths = temp_paths("builtin");
        create_private_dir(paths.config_root()).unwrap();
        let first = sync(&paths).unwrap();
        assert!(first.written.contains(&"modes/PLAN.md".to_owned()));
        assert!(
            sync(&paths).unwrap() == Synced::default(),
            "nothing to do twice"
        );

        // An older build wrote PLAN.md; this build ships another text.
        let plan = paths.modes_dir().join("PLAN.md");
        let mut lock = read_lock(&paths.config_root().join(LOCK_FILE));
        fs::write(&plan, "old plan").unwrap();
        lock.insert("modes/PLAN.md".into(), hash(b"old plan"));
        write(
            &paths.config_root().join(LOCK_FILE),
            &serde_json::to_vec(&lock).unwrap(),
        )
        .unwrap();
        // The user changed MAIN.md.
        let main = paths.main_prompt();
        fs::write(&main, "my main").unwrap();

        let synced = sync(&paths).unwrap();
        assert_eq!(synced.updated, ["modes/PLAN.md"]);
        assert_eq!(fs::read_to_string(&plan).unwrap(), modes::PLAN_TEMPLATE);
        assert_eq!(synced.kept, ["modes/MAIN.md"]);
        assert_eq!(fs::read_to_string(&main).unwrap(), "my main");
        let beside = paths.modes_dir().join("MAIN.md.new");
        assert_eq!(fs::read_to_string(beside).unwrap(), modes::MAIN_TEMPLATE);
        assert!(sync(&paths).unwrap().kept.is_empty(), "told once");

        let listed = skills::list(&paths.skills_dir(), &Config::parse(b"{}").unwrap());
        let broken: Vec<_> = listed
            .iter()
            .filter(|skill| skill.problem.is_some())
            .collect();
        assert!(broken.is_empty(), "{broken:?}");
        assert!(
            listed.iter().all(|skill| skill.source.is_some()),
            "every one is credited"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = paths
                .skills_dir()
                .join("gh-fix-ci/scripts/inspect_pr_checks.py");
            let mode = fs::metadata(script).unwrap().permissions().mode();
            assert_ne!(mode & 0o111, 0);
        }
    }
}
