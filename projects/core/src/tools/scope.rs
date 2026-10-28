//! Where tools may act: the directory the client was started in and the session workspace.
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
                .context("session workspace is not accessible")?,
        })
    }

    /// Relative paths are resolved against.
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Commands run here; temporary and downloaded files belong here.
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Resolves a path the model gave, existing or not, and checks that it stays inside
    /// the working directory or the workspace.
    pub fn resolve(&self, path: &str) -> Result<PathBuf> {
        ensure!(!path.trim().is_empty(), "path is empty");
        let path = self.cwd.join(path);
        let resolved = resolve_existing_prefix(&path)?;
        ensure!(
            resolved.starts_with(&self.cwd) || resolved.starts_with(&self.workspace),
            "path is outside the working directory and the session workspace"
        );
        Ok(resolved)
    }
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
    fn resolves_inside_both_roots() {
        let (scope, _) = scope("scope_inside");
        assert_eq!(
            scope.resolve("a/b.txt").unwrap(),
            scope.cwd().join("a/b.txt")
        );
        let in_workspace = scope.workspace().join("x");
        assert_eq!(
            scope.resolve(in_workspace.to_str().unwrap()).unwrap(),
            in_workspace
        );
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
}
