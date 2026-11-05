//! Where tools may act: the directory the client was started in, the thread workspace and
//! the skills and agents folders, so agents can write and improve them. Never the rest of
//! the config folder, which holds the API keys and the settings that bound the agent.
//! Paths are resolved after following symlinks, so neither `..` nor a link leads outside.

use std::{
    io::ErrorKind,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolScope {
    cwd: PathBuf,
    workspace: PathBuf,
    protected: Option<PathBuf>,
    /// Folders open to the tools wherever they are, the protected one included.
    shared: Vec<PathBuf>,
    /// The launcher-selected skill root, for reporting writes without guessing paths.
    skills: Option<PathBuf>,
}

impl ToolScope {
    /// Both directories must exist.
    pub fn new(cwd: &Path, workspace: &Path) -> Result<Self> {
        Ok(Self {
            cwd: cwd
                .canonicalize()
                .context("working directory is not accessible")?,
            workspace: workspace
                .canonicalize()
                .context("thread workspace is not accessible")?,
            protected: None,
            shared: Vec::new(),
            skills: None,
        })
    }

    /// Keeps the tools out of `folder` and everything under it.
    pub fn protect(mut self, folder: &Path) -> Self {
        self.protected = folder.canonicalize().ok();
        self
    }

    /// Opens `folder` to the tools, even inside the protected one. A missing folder
    /// stays closed.
    pub fn share(mut self, folder: &Path) -> Self {
        self.shared.extend(folder.canonicalize().ok());
        self
    }

    /// Identifies skills for file-write notices; does not grant access to the folder.
    pub(crate) fn with_skills(mut self, folder: &Path) -> Self {
        self.skills = folder.canonicalize().ok();
        self
    }

    pub(crate) fn skill_name(&self, path: &Path) -> Option<String> {
        crate::catalog::graph::node_name(
            self.skills.as_ref()?,
            crate::catalog::graph::SKILL_FILE,
            path,
        )
    }

    /// The folder no tool may touch, the config folder with the keys.
    pub fn protected(&self) -> Option<&Path> {
        self.protected.as_deref()
    }

    /// Commands run here and relative paths are resolved against it.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Temporary and downloaded files belong here.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Resolves a path the model gave, existing or not, and checks that it stays inside
    /// the working directory or the workspace.
    #[cfg(test)]
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        let (resolved, place) = self.locate(path)?;
        ensure!(
            place == Place::Inside,
            "path is outside the working directory and the thread workspace"
        );
        Ok(resolved)
    }

    /// Resolves a path the model gave and says whether it is inside the tools' folders.
    /// The config folder is an error wherever it is: no approval opens it.
    pub fn locate(&self, path: &str) -> Result<(PathBuf, Place)> {
        ensure!(!path.trim().is_empty(), "path is empty");
        let resolved = resolve_existing_prefix(&self.cwd.join(path))?;
        if self
            .shared
            .iter()
            .any(|shared| resolved.starts_with(shared))
        {
            return Ok((resolved, Place::Inside));
        }
        ensure!(
            !self
                .protected
                .as_ref()
                .is_some_and(|protected| resolved.starts_with(protected)),
            "path is inside the dusk config folder, which tools may not touch"
        );
        let place = match resolved.starts_with(&self.cwd) || resolved.starts_with(&self.workspace) {
            true => Place::Inside,
            false => Place::Outside,
        };
        Ok((resolved, place))
    }
}

/// Where a resolved path lies: inside the tools' folders, or outside, where a tool needs
/// the user's approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Inside,
    Outside,
}

/// Canonicalizes the longest existing prefix and appends the rest, which must be plain names.
fn resolve_existing_prefix(path: &Path) -> Result<PathBuf> {
    let mut missing = Vec::new();
    let mut existing = path;
    loop {
        match existing.canonicalize() {
            Ok(base) => {
                return Ok(missing
                    .iter()
                    .rev()
                    .fold(base, |path, name| path.join(name)));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                // A dangling symlink also reports NotFound; writing through it would
                // create its target, wherever that is.
                ensure!(
                    std::fs::symlink_metadata(existing).is_err(),
                    "path is a symlink to a missing target"
                );
                let name = match existing.components().next_back() {
                    Some(Component::Normal(name)) => name,
                    _ => bail!("path cannot be resolved"),
                };
                missing.push(name.to_owned());
                existing = existing.parent().context("path cannot be resolved")?;
            }
            Err(_) => bail!("path is not accessible"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::temp_dir;

    fn scope(name: &str) -> (ToolScope, PathBuf) {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        (ToolScope::new(&cwd, &workspace).unwrap(), root)
    }

    #[test]
    fn rejects_escapes() {
        let (scope, root) = scope("scope_escape");
        for path in ["../outside", "/etc/passwd", "", "a/../../outside"] {
            assert!(scope.resolve(path).is_err(), "{path:?}");
        }
        fs::write(root.join("secret"), "x").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("secret"), scope.cwd().join("link")).unwrap();
            assert!(scope.resolve("link").is_err(), "symlink out");
            std::os::unix::fs::symlink(root.join("missing"), scope.cwd().join("dangling")).unwrap();
            assert!(scope.resolve("dangling").is_err(), "dangling symlink out");
            assert!(
                scope.resolve("dangling/file").is_err(),
                "through a dangling symlink"
            );
        }
    }

    /// Outside the folders a path waits for approval; the config folder never does.
    #[test]
    fn the_config_folder_is_closed_outside_the_folders_too() {
        let (scope, root) = scope("scope_outside");
        fs::create_dir_all(root.join("config")).unwrap();
        let scope = scope.protect(&root.join("config"));
        let (_, place) = scope.locate("../elsewhere/file").unwrap();
        assert_eq!(place, Place::Outside);
        assert!(scope.resolve("../elsewhere/file").is_err());
        assert!(scope.locate("../config/config.yml").is_err());
    }

    #[test]
    fn the_config_folder_is_off_limits_inside_the_cwd() {
        let (scope, _) = scope("scope_config");
        fs::create_dir_all(scope.cwd().join(".config/duskcode")).unwrap();
        let scope = scope.clone().protect(&scope.cwd().join(".config/duskcode"));
        assert!(scope.resolve(".config/duskcode/config.yml").is_err());
        assert!(
            scope
                .resolve(".config/duskcode/../duskcode/skills/x")
                .is_err()
        );
        assert!(scope.resolve(".config/other.yml").is_ok());

        let skills = scope.cwd().join(".config/duskcode/skills");
        fs::create_dir_all(&skills).unwrap();
        let elsewhere = ToolScope::new(scope.workspace(), scope.workspace())
            .unwrap()
            .protect(&scope.cwd().join(".config/duskcode"))
            .share(&skills);
        assert!(
            elsewhere
                .resolve(&format!("{}/x/SKILL.md", skills.display()))
                .is_ok()
        );
        assert!(
            elsewhere
                .resolve(&format!("{}/../config.yml", skills.display()))
                .is_err()
        );
    }
}
