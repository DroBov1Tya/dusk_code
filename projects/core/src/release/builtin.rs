//! The files dusk ships into the rules folder: the prompts of the modes, COMPACT.md,
//! FORK.md, built-in skills and specialist agents. They are the repository's: at every
//! start each one whose hash differs from this build's is replaced with it, a local edit
//! included. Files the user added beside them are never touched.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};

use crate::{init::create_private_dir, init::paths::Paths, release::resources};

/// Kept edited shipped files by their hash in older builds.
pub const LOCK_FILE: &str = "builtin.lock";
const OLD_NEW_SUFFIX: &str = ".new";

/// What a start did to the shipped files, by their path in the config folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    pub written: Vec<String>,
    /// Differed from this build, by an older build or a local edit, and were replaced.
    pub updated: Vec<String>,
}

pub fn sync(paths: &Paths) -> Result<Synced> {
    let root = paths.rules_root();
    let root = root.as_path();
    // Left by builds that kept edited shipped files; nothing reads it now.
    let _ = fs::remove_file(root.join(LOCK_FILE));
    let mut synced = Synced::default();
    for (path, contents) in shipped()? {
        let target = root.join(&path);
        safe_parent(root, &target)?;
        // A copy those builds put beside an edited file.
        let _ = fs::remove_file(format!("{}{OLD_NEW_SUFFIX}", target.display()));
        let on_disk = match fs::symlink_metadata(&target) {
            // A link was put there by hand; dusk does not write through it.
            Ok(metadata) if !metadata.is_file() => continue,
            Ok(_) => Some(fs::read(&target).context("cannot read a shipped file")?),
            Err(error) if error.kind() == ErrorKind::NotFound => None,
            Err(_) => continue,
        };
        match on_disk {
            None => {
                write(&target, &contents)?;
                synced.written.push(path);
            }
            Some(disk) if hash(&disk) != hash(&contents) => {
                write(&target, &contents)?;
                synced.updated.push(path);
            }
            Some(_) => {}
        }
    }
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
                ("internal", _) => Path::new("prompts").join(&relative),
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
    use crate::{catalog::skills, config::Config, testing::temp_paths};

    #[test]
    fn shipped_files_follow_the_build_and_added_files_stay() {
        let paths = temp_paths("builtin");
        create_private_dir(&paths.rules_root()).unwrap();
        let first = sync(&paths).unwrap();
        assert!(first.written.contains(&"modes/PLAN.md".to_owned()));
        assert!(
            sync(&paths).unwrap() == Synced::default(),
            "nothing to do twice"
        );

        // An older build wrote PLAN.md, the user edited MAIN.md and added a mode.
        let plan = paths.modes_dir().join("PLAN.md");
        fs::write(&plan, "old plan").unwrap();
        let main = paths.main_prompt();
        fs::write(&main, "my main").unwrap();
        fs::write(format!("{}.new", main.display()), "left over").unwrap();
        let mine = paths.modes_dir().join("MINE.md");
        fs::write(&mine, "my mode").unwrap();

        let mut updated = sync(&paths).unwrap().updated;
        updated.sort();
        assert_eq!(updated, ["modes/MAIN.md", "modes/PLAN.md"]);
        let shipped = |name| crate::catalog::modes::template(name).unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(&plan).unwrap(),
            shipped(crate::catalog::modes::PLAN)
        );
        assert_eq!(
            fs::read_to_string(&main).unwrap(),
            shipped(crate::catalog::modes::MAIN)
        );
        assert!(!paths.modes_dir().join("MAIN.md.new").exists());
        assert_eq!(fs::read_to_string(&mine).unwrap(), "my mode");

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
