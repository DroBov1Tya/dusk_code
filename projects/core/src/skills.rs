//! Skills as a client manages them: every folder under `skills/` with a `SKILL.md`, its
//! switch, and its text to read and edit. Skills are read from disk each time, so a
//! skill added or edited by hand applies at once and nothing is kept elsewhere.
//!
//! A skill is one of three kinds. Built-in skills ship inside dusk and are written to
//! `skills/` when missing; each credits its authors in its front matter and keeps their
//! license beside it. A skill whose front matter names a `source` came from elsewhere.
//! Every other skill is the user's own.

use std::{fs, path::Path};

use include_dir::{Dir, DirEntry, include_dir};

use anyhow::{Context, Result};

use crate::{
    config::Config,
    graph::{Graph, SKILL_FILE},
};

/// The skills dusk ships, with their licenses and `NOTICE.md`.
static BUILTIN: Dir = include_dir!("$CARGO_MANIFEST_DIR/skills");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillKind {
    Builtin,
    Own,
    Other,
}

impl SkillKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Builtin => "built-in",
            Self::Own => "own",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub kind: SkillKind,
    /// The company or project it comes from, as its front matter says.
    pub source: Option<String>,
    pub tags: Vec<String>,
    pub enabled: bool,
    /// Why the skill cannot be used as it is; it can still be opened and fixed.
    pub problem: Option<String>,
}

/// Every skill, the ones that cannot be read included, by name.
pub fn list(directory: &Path, config: &Config) -> Vec<SkillInfo> {
    let graph = Graph::load(directory, SKILL_FILE);
    let mut skills: Vec<SkillInfo> = graph
        .nodes()
        .map(|node| SkillInfo {
            name: node.name.clone(),
            description: node.description.clone(),
            kind: kind(&node.name, node.source.is_some()),
            source: node.source.clone(),
            tags: node.tags.clone(),
            enabled: config.skill_enabled(&node.name),
            problem: None,
        })
        .collect();
    for problem in graph.problems() {
        if graph.get(&problem.node).is_none() {
            skills.push(SkillInfo {
                name: problem.node.clone(),
                description: String::new(),
                kind: kind(&problem.node, false),
                source: None,
                tags: Vec::new(),
                enabled: config.skill_enabled(&problem.node),
                problem: Some(problem.reason.clone()),
            });
        }
    }
    skills.sort_by(|a, b| (a.kind, &a.name).cmp(&(b.kind, &b.name)));
    skills
}

/// Writes every built-in skill that `directory` lacks, and the notice that credits them.
/// A skill already there is left alone, edited or not. Returns the skills written.
pub fn install_builtin(directory: &Path) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for entry in BUILTIN.entries() {
        let target = directory.join(entry.path());
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        write_entry(directory, entry)?;
        if let DirEntry::Dir(dir) = entry {
            written.push(dir.path().display().to_string());
        }
    }
    Ok(written)
}

fn write_entry(directory: &Path, entry: &DirEntry) -> Result<()> {
    let target = directory.join(entry.path());
    match entry {
        DirEntry::Dir(dir) => {
            fs::create_dir_all(&target)
                .with_context(|| format!("cannot create {}", target.display()))?;
            for entry in dir.entries() {
                write_entry(directory, entry)?;
            }
        }
        DirEntry::File(file) => {
            fs::write(&target, file.contents())
                .with_context(|| format!("cannot write {}", target.display()))?;
            // The archive keeps no file modes; a script says it is one by its first line.
            #[cfg(unix)]
            if file.contents().starts_with(b"#!") {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&target, fs::Permissions::from_mode(0o755))
                    .with_context(|| format!("cannot make {} runnable", target.display()))?;
            }
        }
    }
    Ok(())
}

fn kind(name: &str, has_source: bool) -> SkillKind {
    if BUILTIN.get_dir(name).is_some() {
        SkillKind::Builtin
    } else if has_source {
        SkillKind::Other
    } else {
        SkillKind::Own
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn built_in_skills_install_once_and_all_read() {
        let directory = temp_dir("skills_builtin");
        fs::create_dir_all(directory.join("spike")).unwrap();
        fs::write(directory.join("spike").join(SKILL_FILE), "kept").unwrap();
        let written = install_builtin(&directory).unwrap();
        assert!(
            !written.contains(&"spike".to_owned()),
            "an existing skill is kept"
        );
        assert_eq!(
            fs::read_to_string(directory.join("spike").join(SKILL_FILE)).unwrap(),
            "kept"
        );
        assert!(directory.join("NOTICE.md").is_file());
        assert!(install_builtin(&directory).unwrap().is_empty());

        fs::remove_dir_all(directory.join("spike")).unwrap();
        install_builtin(&directory).unwrap();
        let skills = list(&directory, &Config::parse(b"{}").unwrap());
        let broken: Vec<_> = skills
            .iter()
            .filter(|skill| skill.problem.is_some())
            .collect();
        assert!(broken.is_empty(), "{broken:?}");
        assert!(skills.iter().all(|skill| skill.kind == SkillKind::Builtin));
        assert!(
            skills.iter().all(|skill| skill.source.is_some()),
            "every one is credited"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = directory.join("gh-fix-ci/scripts/inspect_pr_checks.py");
            assert_ne!(
                fs::metadata(script).unwrap().permissions().mode() & 0o111,
                0
            );
        }
    }
}
