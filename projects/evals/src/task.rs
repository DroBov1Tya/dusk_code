//! A task is a folder: `task.md` says what to do, `fixture/` is the project, `setup.sh`
//! may add to it, `check.sh` decides, and `solution.sh` is a known way to pass.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

pub struct Task {
    pub name: String,
    pub prompt: String,
    pub folder: PathBuf,
}

pub fn load(tasks: &Path, only: &[String]) -> Result<Vec<Task>> {
    let mut found = Vec::new();
    let entries = fs::read_dir(tasks)
        .with_context(|| format!("cannot read {}; run from the repository", tasks.display()))?;
    for entry in entries {
        let folder = entry?.path();
        let name = folder
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if !folder.join("task.md").is_file() || !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let prompt = fs::read_to_string(folder.join("task.md"))?;
        found.push(Task {
            name,
            prompt: prompt.trim().to_owned(),
            folder,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    ensure!(!found.is_empty(), "no tasks found");
    Ok(found)
}
