//! Which data roots two paths share, resolved through links.

use std::{
    fs::{self},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result};

pub fn absolute_for_overlap(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_owned());
    }
    Ok(std::env::current_dir()
        .context(
            "cannot resolve a relative data path because the current directory is unavailable",
        )?
        .join(path))
}

pub fn paths_overlap(left: &Path, right: &Path) -> bool {
    if left.starts_with(right) || right.starts_with(left) {
        return true;
    }
    match (physical_path(left), physical_path(right)) {
        (Some(left), Some(right)) => left.starts_with(&right) || right.starts_with(&left),
        _ => false,
    }
}

/// Resolve existing ancestors too: a symlink alias must still coordinate when the
/// final data root has not been created yet. A disjoint inaccessible HOME is harmless.
pub fn physical_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if let Ok(mut physical) = fs::canonicalize(ancestor) {
            // Preserve symlink semantics in the existing prefix, then normalize the
            // missing suffix. A missing `sub/../state.db` still belongs to its parent.
            for component in path.strip_prefix(ancestor).ok()?.components() {
                match component {
                    Component::Normal(name) => physical.push(name),
                    Component::CurDir => {}
                    Component::ParentDir => {
                        physical.pop();
                    }
                    Component::Prefix(_) | Component::RootDir => return None,
                }
            }
            return Some(physical);
        }
    }
    None
}
