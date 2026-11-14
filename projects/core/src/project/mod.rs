//! Projects: folders the user names. A client sees git and files only inside a saved
//! project's folder, and only through the paths `inside` lets through.

mod diff;
mod edit;
pub mod env;
mod files;
mod git;
mod outputs;
pub mod schedule;

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::project::{MAX_DESCRIPTION_CHARS, MAX_NAME_CHARS, Project};

use crate::state::State;

pub use diff::diff;
pub use edit::{read as read_file, write as write_file};
pub use files::list as files;
pub use git::{changes as git_changes, info as git, summary as git_summary, switch_branch};
pub use outputs::{delete as delete_output, list as outputs, read as read_output};

/// Saves a new project, or changes the one with `id`. The folder is stored resolved, so
/// one folder reached by two paths is still one project.
pub async fn save(
    state: &State,
    id: Option<String>,
    name: &str,
    description: &str,
    dir: &Path,
) -> Result<Project> {
    let name = name.trim();
    ensure!(!name.is_empty(), "a project needs a name");
    ensure!(
        name.chars().count() <= MAX_NAME_CHARS,
        "a project name is at most {MAX_NAME_CHARS} characters"
    );
    let description = description.trim();
    ensure!(
        description.chars().count() <= MAX_DESCRIPTION_CHARS,
        "a project description is at most {MAX_DESCRIPTION_CHARS} characters"
    );
    ensure!(
        dir.is_absolute(),
        "the project folder must be an absolute path"
    );
    let dir = tokio::fs::canonicalize(dir)
        .await
        .with_context(|| format!("cannot open {}", dir.display()))?;
    ensure!(dir.is_dir(), "{} is not a folder", dir.display());
    let shown = dir
        .to_str()
        .context("the project folder is not valid UTF-8")?;
    let id = match id {
        Some(id) => {
            ensure!(
                state.update_project(&id, name, description, shown).await?,
                "no such project"
            );
            id
        }
        None => {
            let id = uuid::Uuid::now_v7().to_string();
            state.create_project(&id, name, description, shown).await?;
            id
        }
    };
    state
        .load_project(&id)
        .await?
        .context("the project was not stored")
}

pub async fn find(state: &State, id: &str) -> Result<Project> {
    state.load_project(id).await?.context("no such project")
}

/// `relative` inside the project folder, resolved. Refused when it is absolute, climbs
/// with `..`, or leads out through a symbolic link.
async fn inside(project: &Project, relative: &str) -> Result<PathBuf> {
    let relative = Path::new(relative);
    for part in relative.components() {
        match part {
            Component::Normal(_) | Component::CurDir => {}
            _ => bail!("the path must stay inside the project"),
        }
    }
    let path = tokio::fs::canonicalize(project.dir.join(relative))
        .await
        .context("no such path in the project")?;
    ensure!(
        path.starts_with(&project.dir),
        "the path must stay inside the project"
    );
    Ok(path)
}

/// `path` resolved, links followed, when it is inside one of `roots`: the one check for
/// what a client may name by an absolute path.
pub async fn within(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let path = tokio::fs::canonicalize(path)
        .await
        .context("no such path")?;
    for root in roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await
            && path.starts_with(root)
        {
            return Ok(path);
        }
    }
    bail!("the path must be inside the workspace or a project")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{temp_dir, temp_state};

    fn project(dir: PathBuf) -> Project {
        Project {
            id: "p".into(),
            name: "p".into(),
            description: String::new(),
            dir,
            created_at: 0,
        }
    }

    #[tokio::test]
    async fn paths_stay_inside() {
        let root = temp_dir("project-inside");
        std::fs::create_dir_all(root.join("p/src")).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let dir = root.join("p");
        let project = project(dir.clone());
        assert_eq!(inside(&project, "src").await.unwrap(), dir.join("src"));
        assert_eq!(inside(&project, "").await.unwrap(), dir);
        for bad in ["..", "src/../..", "/etc", "../p"] {
            assert!(inside(&project, bad).await.is_err(), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root, dir.join("out")).unwrap();
            assert!(inside(&project, "out").await.is_err());
        }
    }

    #[tokio::test]
    async fn a_project_is_checked_before_it_is_saved() {
        let state = temp_state("project-save").await;
        let root = temp_dir("project-folder");
        std::fs::create_dir_all(&root).unwrap();
        assert!(save(&state, None, " ", "", &root).await.is_err());
        assert!(
            save(&state, None, "a", "", Path::new("relative"))
                .await
                .is_err()
        );
        assert!(
            save(&state, None, "a", "", &root.join("none"))
                .await
                .is_err()
        );
        let saved = save(&state, None, " a ", "", &root).await.unwrap();
        assert_eq!(saved.name, "a");
        assert_eq!(saved.dir, std::fs::canonicalize(&root).unwrap());
        assert!(
            save(&state, Some("other".into()), "a", "", &root)
                .await
                .is_err()
        );
    }
}
