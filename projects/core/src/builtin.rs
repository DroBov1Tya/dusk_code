//! The files dusk ships into the config folder: the prompts of the modes, COMPACT.md,
//! FORK.md, built-in skills and specialist agents. At every start each is brought to
//! the version of this build, without losing what the user wrote: `builtin.lock`
//! records the hash of each file as dusk last wrote it. A file still as written is
//! replaced; a file the user changed, or one from before the record, is kept, and
//! the new version is put beside it as `<file>.new`.

use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::{init::create_private_dir, paths::Paths, resources};

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
    for (path, contents) in shipped()? {
        let target = root.join(&path);
        safe_parent(root, &target)?;
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
fn shipped() -> Result<Vec<(String, Vec<u8>)>> {
    let root = resources::root()?;
    let mut files = Vec::new();
    for category in ["agents", "skills", "internal", "modes"] {
        for relative in resources::files(&root.join(category))? {
            let source = root.join(category).join(&relative);
            let path = match (category, relative.to_str()) {
                // Keep the existing editable paths and their hash-lock records.
                ("internal", Some("COMPACT.md" | "FORK.md")) => relative.clone(),
                _ => Path::new(category).join(&relative),
            };
            files.push((path.to_string_lossy().into_owned(), fs::read(source)?));
        }
    }
    Ok(files)
}

fn safe_parent(root: &Path, target: &Path) -> Result<()> {
    let parent = target.parent().context("shipped file has no parent")?;
    let relative = parent
        .strip_prefix(root)
        .context("shipped file is outside config")?;
    create_private_dir(root)?;
    let mut directory = root.to_owned();
    for component in relative.components() {
        directory.push(component);
        create_private_dir(&directory)?;
    }
    Ok(())
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
    let temporary = target.with_file_name(format!(".duskcode-writing-{}", uuid::Uuid::now_v7()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
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
        assert_eq!(
            fs::read_to_string(&plan).unwrap(),
            crate::modes::template(crate::modes::PLAN).unwrap().unwrap()
        );
        assert_eq!(synced.kept, ["modes/MAIN.md"]);
        assert_eq!(fs::read_to_string(&main).unwrap(), "my main");
        let beside = paths.modes_dir().join("MAIN.md.new");
        assert_eq!(
            fs::read_to_string(beside).unwrap(),
            crate::modes::template(crate::modes::MAIN).unwrap().unwrap()
        );
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
