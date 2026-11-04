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
/// Files older builds shipped and this one does not, by path in the rules folder, with
/// the SHA-256 they were shipped with. One still as shipped is removed at start; one the
/// user changed is theirs and stays.
const DROPPED: [(&str, &str); 44] = [
    (
        "skills/frontend-design/LICENSE.txt",
        "0d542e0c8804e39aa7f37eb00da5a762149dc682d7829451287e11b938e94594",
    ),
    (
        "skills/gh-address-comments/LICENSE.txt",
        "58d1e17ffe5109a7ae296caafcadfdbe6a7d176f0bc4ab01e12a689b0499d8bd",
    ),
    (
        "skills/gh-address-comments/scripts/fetch_comments.py",
        "d7bc64f6b26f7482f9ca9dd7c923a5b8b20e1ef13491ae2d9052cd208a7c1ad1",
    ),
    (
        "skills/gh-fix-ci/LICENSE.txt",
        "4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9",
    ),
    (
        "skills/gh-fix-ci/scripts/inspect_pr_checks.py",
        "9459e5b03f86785d184a83e5cd8d621b832e918ec04158a2962d867785812c6c",
    ),
    (
        "skills/jupyter-notebook/LICENSE.txt",
        "4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9",
    ),
    (
        "skills/jupyter-notebook/assets/experiment-template.ipynb",
        "25f85914103cd84244e20ef378f59097479c72481cd18d3a931354d107c3f300",
    ),
    (
        "skills/jupyter-notebook/assets/tutorial-template.ipynb",
        "704a809c09c7c9e6e5c437860df95524412b50c58571a14204610d73e0744765",
    ),
    (
        "skills/jupyter-notebook/scripts/new_notebook.py",
        "a004e2ea3115edb24313928be0be6d1dc07e571d7ca6c1b24375f0569c616d2f",
    ),
    (
        "skills/mcp-builder/LICENSE.txt",
        "bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362",
    ),
    (
        "skills/mcp-builder/scripts/connections.py",
        "9403668a2041568772082a8b334122c1f88daf0541fb393af4522d0094a47a6e",
    ),
    (
        "skills/mcp-builder/scripts/evaluation.py",
        "49ed1d17cdce5da101b210197740713f49b935c29d4f339542a14b132658e6f7",
    ),
    (
        "skills/mcp-builder/scripts/example_evaluation.xml",
        "9272b348ddcc4b06ba562367ccd0770e018158c0068ac5116d5e34aaeff8777a",
    ),
    (
        "skills/mcp-builder/scripts/requirements.txt",
        "d5d7558b2368ecea9dfeed7d1fbc71ee9e0750bebd1282faa527d528a344c3c7",
    ),
    (
        "skills/playwright/LICENSE.txt",
        "9a7110fc2d2f964038e5dc49128f908f29f47a574c961cba16085914e879cbda",
    ),
    (
        "skills/playwright/NOTICE.txt",
        "417d5d3372914ecfd9fe102ac1d7dc2119253209f9718d6c222f616876c1904b",
    ),
    (
        "skills/playwright/scripts/playwright_cli.sh",
        "aa3fdff5d0e4556177f4dfd5f04117e772aa54f94b6a2e34b6c0edf629c6b9b5",
    ),
    (
        "skills/security-best-practices/LICENSE.txt",
        "4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9",
    ),
    (
        "skills/security-threat-model/LICENSE.txt",
        "4dd13869245e356246a5b770723247bbb80a8f07a181d1d3d873a1734297cdb9",
    ),
    (
        "skills/skill-creator/LICENSE.txt",
        "bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362",
    ),
    (
        "skills/skill-creator/agents/analyzer.md",
        "bf68f4cac5a56c673a928c2e6d619586c5b93ea364026ab37547772cb45a663a",
    ),
    (
        "skills/skill-creator/agents/comparator.md",
        "fe1fc9787c495d864c5d6eada47396478572325fde1b33a96d78bf4b849b7a3e",
    ),
    (
        "skills/skill-creator/agents/grader.md",
        "57134da0c1a4eea33fbd74a1c9c44aa814f07d6bc64de303edb586f941e5d21a",
    ),
    (
        "skills/skill-creator/assets/eval_review.md",
        "1c07e65eecc07e63bac14a5a2f8e963fde1dd1bad27c5cc16eb80821fa61f9d2",
    ),
    (
        "skills/skill-creator/eval-viewer/generate_review.py",
        "38c8018e6df847d4d6a5b342a6d74d3c5d17bfcb3e2e838941434838422d126b",
    ),
    (
        "skills/skill-creator/references/schemas.md",
        "3f35325f9292c32bc04ff3eeb340a59cd4d9143ba395337f03c883ee400c2d84",
    ),
    (
        "skills/skill-creator/scripts/__init__.py",
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    ),
    (
        "skills/skill-creator/scripts/aggregate_benchmark.py",
        "6d1644d65d556bc01cce8f2c95af56929f77b2dd9ee37c5c9cbed021c6224ef6",
    ),
    (
        "skills/skill-creator/scripts/generate_report.py",
        "7cb37323b521f3479d86c9dcc83e73580f9198d9d5a9cd2e726b64c9384b22f8",
    ),
    (
        "skills/skill-creator/scripts/improve_description.py",
        "87d864570220b699fac52da309d2d6efdb060647bfebc74f768128e646accf80",
    ),
    (
        "skills/skill-creator/scripts/package_skill.py",
        "1a33059b0db1ef73375d46d513e5ea81369d2e8838c970597b0d52ddef8d1c0f",
    ),
    (
        "skills/skill-creator/scripts/quick_validate.py",
        "67cf5703402013936c8fb75ad6a1afecd8841d45cc5e606b634eb05825fde365",
    ),
    (
        "skills/skill-creator/scripts/run_eval.py",
        "43e3b8f80dbf69c343967ba77e268fae991d9fa3ed68b32a0ff02532cd48657f",
    ),
    (
        "skills/skill-creator/scripts/run_loop.py",
        "648d1e0dccd9f919b43c54bb4caf119c5f75cf495df71369fba5f27880f21d09",
    ),
    (
        "skills/skill-creator/scripts/utils.py",
        "3af8ae62c40c73ab712207436a0d9a981e845f25c5a7040229eb189cc8e45bb1",
    ),
    (
        "skills/spike/LICENSE.txt",
        "f27db279e3df709293a13070ccfe12e63cd0bc579be9d4fe3bde57d7c9bd5868",
    ),
    (
        "skills/systematic-debugging/LICENSE.txt",
        "833ebec48ae8af8dd890d4ba9ce95880cf1235e1063ac0fc12237777829032be",
    ),
    (
        "skills/test-driven-development/LICENSE.txt",
        "833ebec48ae8af8dd890d4ba9ce95880cf1235e1063ac0fc12237777829032be",
    ),
    (
        "skills/webapp-testing/LICENSE.txt",
        "bc6b3af2f331cbc7fb0da1344efb2cbe5877a31498b4d70dbc7000f3405a1362",
    ),
    (
        "skills/webapp-testing/examples/console_logging.py",
        "ea46877289acb82da7e7ce59d0bc37c8977cd57e2a006d0c88d7a1c625bf95da",
    ),
    (
        "skills/webapp-testing/examples/element_discovery.py",
        "d63c89604a22f8845d724e95dda45db49b1bf57c25ce0a83afbb7b8da3d402f0",
    ),
    (
        "skills/webapp-testing/examples/static_html_automation.py",
        "9d533aafb875ee3ab8b8ebf8f5b9003ac8d999da3d09b285cce252e623140064",
    ),
    (
        "skills/webapp-testing/scripts/with_server.py",
        "b0dcf4918935b795f4eda9821579b9902119235ff4447f687a30286e7d0925fd",
    ),
    (
        "prompts/RETIRED_SKILL_FILES.md",
        "373c98debfc2c6dd9220d1b405160ab21b706113e1c9e317b615d565abf9bccf",
    ),
];
/// In the release only so that 0.70.2, which wants it there, can update; never shipped.
const FOR_OLD_UPDATERS: [&str; 1] = ["internal/RETIRED_SKILL_FILES.md"];
/// Where builds before this one moved such files instead of removing them.
const OLD_RECOVERY: &str = "retired-skills";

/// What a start did to the shipped files, by their path in the config folder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Synced {
    pub written: Vec<String>,
    /// Differed from this build, by an older build or a local edit, and were replaced.
    pub updated: Vec<String>,
    /// Shipped by an older build, no longer by this one, and removed.
    pub removed: Vec<String>,
}

impl Synced {
    pub fn sync(paths: &Paths) -> Result<Self> {
        let root = paths.rules_root();
        let root = root.as_path();
        // Left by builds that kept edited shipped files; nothing reads it now.
        let _ = fs::remove_file(root.join(LOCK_FILE));
        let mut synced = Self::default();
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
        synced.drop_old(root, &DROPPED)?;
        Ok(synced)
    }

    /// Removes the files of `dropped` that are still as shipped, in the rules folder and
    /// in the recovery folder older builds moved them to, then the folders left empty.
    fn drop_old(&mut self, root: &Path, dropped: &[(&str, &str)]) -> Result<()> {
        let recovery = root.join(OLD_RECOVERY);
        let moved: Vec<_> = match fs::read_dir(&recovery) {
            Ok(entries) => entries.flatten().map(|entry| entry.path()).collect(),
            Err(_) => Vec::new(),
        };
        for &(relative, shipped_hash) in dropped {
            let relative = Path::new(relative);
            ensure!(
                relative
                    .components()
                    .all(|part| matches!(part, Component::Normal(_))),
                "invalid dropped file path"
            );
            for base in std::iter::once(root).chain(moved.iter().map(|path| path.as_path())) {
                let Some(file) = regular_file(base, relative) else {
                    continue;
                };
                let metadata = fs::symlink_metadata(&file)?;
                let contents = crate::util::fs::read_regular(&file, metadata.len())?;
                if hash(&contents) != shipped_hash {
                    continue;
                }
                fs::remove_file(&file).context("cannot remove an old shipped file")?;
                self.removed.push(
                    file.strip_prefix(root)
                        .unwrap_or(&file)
                        .to_string_lossy()
                        .into_owned(),
                );
                // A folder that still holds anything stays.
                let mut folder = file.parent();
                while let Some(parent) = folder.filter(|parent| *parent != root) {
                    if fs::remove_dir(parent).is_err() {
                        break;
                    }
                    folder = parent.parent();
                }
            }
        }
        Ok(())
    }
}

/// `relative` under `base` when it is a regular file reached through no link.
fn regular_file(base: &Path, relative: &Path) -> Option<std::path::PathBuf> {
    let mut path = base.to_owned();
    for part in relative.components() {
        path.push(part);
        if fs::symlink_metadata(&path).ok()?.file_type().is_symlink() {
            return None;
        }
    }
    fs::symlink_metadata(&path).ok()?.is_file().then_some(path)
}

/// Every shipped file by its path in the config folder, with its contents.
fn shipped() -> Result<Vec<(String, Vec<u8>)>> {
    let root = resources::root()?;
    let mut files = Vec::new();
    for category in resources::CATEGORIES {
        for relative in resources::files(&root.join(category))? {
            if FOR_OLD_UPDATERS
                .contains(&Path::new(category).join(&relative).to_str().unwrap_or(""))
            {
                continue;
            }
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
        let first = Synced::sync(&paths).unwrap();
        assert!(first.written.contains(&"modes/PLAN.md".to_owned()));
        assert!(
            !paths
                .rules_root()
                .join("prompts/RETIRED_SKILL_FILES.md")
                .exists()
        );
        assert!(
            Synced::sync(&paths).unwrap() == Synced::default(),
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

        let mut updated = Synced::sync(&paths).unwrap().updated;
        updated.sort();
        assert_eq!(updated, ["modes/MAIN.md", "modes/PLAN.md"]);
        let shipped = |name| {
            crate::catalog::modes::Modes::template(name)
                .unwrap()
                .unwrap()
        };
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
    fn old_shipped_files_go_and_changed_ones_stay() {
        let paths = temp_paths("dropped");
        let root = paths.rules_root();
        let skill = paths.skills_dir().join("legacy");
        let moved = root.join(OLD_RECOVERY).join("0190").join("skills/legacy");
        fs::create_dir_all(&skill).unwrap();
        fs::create_dir_all(&moved).unwrap();
        let shipped = b"bundled helper";
        fs::write(skill.join("old.py"), shipped).unwrap();
        fs::write(skill.join("edited.py"), "user changes").unwrap();
        fs::write(skill.join("SKILL.md"), "the skill").unwrap();
        fs::write(moved.join("old.py"), shipped).unwrap();
        let digest = hash(shipped);
        let mut dropped = vec![
            ("skills/legacy/old.py", digest.as_str()),
            ("skills/legacy/edited.py", digest.as_str()),
        ];
        #[cfg(unix)]
        {
            let outside = temp_paths("dropped-outside").rules_root();
            fs::create_dir_all(&outside).unwrap();
            fs::write(outside.join("old.py"), shipped).unwrap();
            std::os::unix::fs::symlink(&outside, paths.skills_dir().join("linked")).unwrap();
            dropped.push(("skills/linked/old.py", digest.as_str()));
        }
        let mut synced = Synced::default();
        synced.drop_old(&root, &dropped).unwrap();
        assert!(!skill.join("old.py").exists());
        assert_eq!(
            fs::read_to_string(skill.join("edited.py")).unwrap(),
            "user changes"
        );
        assert!(skill.join("SKILL.md").exists());
        assert!(
            !root.join(OLD_RECOVERY).exists(),
            "the emptied recovery goes"
        );
        assert_eq!(synced.removed.len(), 2);
        #[cfg(unix)]
        assert!(paths.skills_dir().join("linked/old.py").exists());
        assert!(
            synced
                .drop_old(&root, &[("skills/../escape.py", "")])
                .is_err()
        );
    }
}
