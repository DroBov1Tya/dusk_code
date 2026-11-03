//! The files dusk ships into the rules folder: the prompts of the modes, COMPACT.md,
//! FORK.md, built-in skills and specialist agents. They are the repository's: at every
//! start each one whose hash differs from this build's is replaced with it, a local edit
//! included. Files the user added beside them are never touched.

use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Component, Path},
};

use anyhow::{Context, Result, ensure};
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
    /// Files moved out of active skills into recovery storage.
    pub removed: Vec<String>,
    /// Recovery paths relative to the rules root; retirement never deletes contents.
    pub archived: Vec<String>,
    /// Retired files with local changes or unsafe paths, left untouched.
    pub preserved: Vec<String>,
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
    retire(
        root,
        &resources::text("internal/RETIRED_SKILL_FILES.md")?,
        &mut synced,
    )?;
    Ok(synced)
}

/// Retire only named historical built-ins that match at inspection time. Keep their
/// contents in recovery storage: a concurrent edit or replacement between the hash
/// check and rename must not cause data loss. No recursive sweep or unlink occurs.
fn retire(root: &Path, manifest: &str, synced: &mut Synced) -> Result<()> {
    for line in manifest
        .lines()
        .filter(|line| line.starts_with("| skills/"))
    {
        let fields: Vec<_> = line.split('|').map(str::trim).collect();
        ensure!(fields.len() == 4, "invalid retired skill entry");
        let (relative, expected) = (Path::new(fields[1]), fields[2]);
        ensure!(
            relative
                .components()
                .all(|part| matches!(part, Component::Normal(_)))
                && relative.starts_with("skills")
                && expected.len() == 64
                && expected.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid retired skill path or hash"
        );
        let mut target = root.to_owned();
        let mut present = true;
        for part in relative.components() {
            target.push(part);
            match fs::symlink_metadata(&target) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    synced.preserved.push(fields[1].to_owned());
                    present = false;
                    break;
                }
                Ok(_) => {}
                Err(error) if error.kind() == ErrorKind::NotFound => {
                    present = false;
                    break;
                }
                Err(error) => return Err(error).context("cannot inspect retired skill file"),
            }
        }
        if !present {
            continue;
        }
        let metadata = fs::symlink_metadata(&target)?;
        if !metadata.is_file() {
            synced.preserved.push(fields[1].to_owned());
            continue;
        }
        let contents = crate::util::fs::read_regular(&target, metadata.len())?;
        if hash(&contents) == expected {
            let archived = Path::new("retired-skills")
                .join(uuid::Uuid::now_v7().to_string())
                .join(relative);
            let recovery = root.join(&archived);
            safe_parent(root, &recovery)?;
            fs::rename(&target, &recovery).context("cannot archive a retired skill file")?;
            synced.removed.push(fields[1].to_owned());
            synced
                .archived
                .push(archived.to_string_lossy().into_owned());
        } else {
            synced.preserved.push(fields[1].to_owned());
        }
    }
    Ok(())
}

/// Every shipped file by its path in the config folder, with its contents.
fn shipped() -> Result<Vec<(String, Vec<u8>)>> {
    let root = resources::root()?;
    let mut files = Vec::new();
    for category in resources::CATEGORIES {
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
        let files = resources::files(&paths.skills_dir()).unwrap();
        assert!(
            files
                .iter()
                .all(|path| path.extension().is_some_and(|ext| ext == "md"))
        );
        let creator =
            fs::read_to_string(paths.skills_dir().join("skill-creator/SKILL.md")).unwrap();
        assert!(creator.contains("source: \"dusk_code\""));
    }

    #[test]
    fn sync_retires_known_old_license_and_preserves_local_helper() {
        let paths = temp_paths("retired-sync");
        let directory = paths.skills_dir().join("gh-fix-ci");
        fs::create_dir_all(directory.join("scripts")).unwrap();
        let license = resources::text("skills/gh-fix-ci/LICENSE.md").unwrap();
        fs::write(directory.join("LICENSE.txt"), license).unwrap();
        fs::write(
            directory.join("scripts/inspect_pr_checks.py"),
            "locally changed",
        )
        .unwrap();
        let synced = sync(&paths).unwrap();
        assert!(
            synced
                .removed
                .contains(&"skills/gh-fix-ci/LICENSE.txt".to_owned())
        );
        assert!(
            synced
                .preserved
                .contains(&"skills/gh-fix-ci/scripts/inspect_pr_checks.py".to_owned())
        );
        assert!(directory.join("LICENSE.md").is_file());
        assert!(!directory.join("LICENSE.txt").exists());
        assert_eq!(
            fs::read_to_string(directory.join("scripts/inspect_pr_checks.py")).unwrap(),
            "locally changed"
        );
    }

    #[test]
    fn retirement_keeps_data_written_through_an_existing_handle() {
        let paths = temp_paths("retired-open-file");
        let directory = paths.skills_dir().join("legacy");
        fs::create_dir_all(&directory).unwrap();
        let target = directory.join("old.py");
        fs::write(&target, "bundled helper").unwrap();
        let mut editor = OpenOptions::new().append(true).open(&target).unwrap();
        let manifest = format!("| skills/legacy/old.py | {} |\n", hash(b"bundled helper"));
        let mut synced = Synced::default();
        retire(&paths.rules_root(), &manifest, &mut synced).unwrap();
        editor.write_all(b" with concurrent changes").unwrap();
        assert!(!target.exists());
        let recovery = paths.rules_root().join(&synced.archived[0]);
        assert_eq!(
            fs::read_to_string(recovery).unwrap(),
            "bundled helper with concurrent changes"
        );
    }

    #[test]
    fn retirement_preserves_modified_added_and_symlinked_files() {
        let paths = temp_paths("retired-skills");
        let root = paths.rules_root();
        let directory = paths.skills_dir().join("legacy");
        fs::create_dir_all(&directory).unwrap();
        let original = b"original bundled helper";
        fs::write(directory.join("old.py"), original).unwrap();
        fs::write(directory.join("edited.py"), "user changes").unwrap();
        fs::write(directory.join("mine.py"), "user helper").unwrap();
        let digest = hash(original);
        let mut manifest = format!(
            "| skills/legacy/old.py | {digest} |\n| skills/legacy/edited.py | {digest} |\n"
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.join("mine.py"), directory.join("link.py"))
                .unwrap();
            std::os::unix::fs::symlink(&directory, paths.skills_dir().join("linked")).unwrap();
            manifest.push_str(&format!(
                "| skills/legacy/link.py | {digest} |\n| skills/linked/mine.py | {} |\n",
                hash(b"user helper")
            ));
        }
        let mut synced = Synced::default();
        retire(&root, &manifest, &mut synced).unwrap();
        assert_eq!(synced.removed, ["skills/legacy/old.py"]);
        assert_eq!(synced.archived.len(), 1);
        assert_eq!(fs::read(root.join(&synced.archived[0])).unwrap(), original);
        assert!(
            synced
                .preserved
                .contains(&"skills/legacy/edited.py".to_owned())
        );
        assert_eq!(
            fs::read_to_string(directory.join("edited.py")).unwrap(),
            "user changes"
        );
        assert_eq!(
            fs::read_to_string(directory.join("mine.py")).unwrap(),
            "user helper"
        );
        #[cfg(unix)]
        assert_eq!(synced.preserved.len(), 3);
        let mut second = Synced::default();
        retire(&root, &manifest, &mut second).unwrap();
        assert!(second.removed.is_empty());
        assert_eq!(second.preserved, synced.preserved);
        assert!(
            retire(
                &root,
                &format!("| skills/../escape.py | {digest} |"),
                &mut second
            )
            .is_err()
        );
    }
}
