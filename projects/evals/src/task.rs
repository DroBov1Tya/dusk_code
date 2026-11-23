//! A task is a folder: `task.md` says what to do, `fixture/` is the project, `setup.sh`
//! may add to it, `check.sh` decides, and `solution.sh` is a known way to pass.
//! `run.yml`, if any, says how the runner runs it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

use crate::run::KillAfter;

pub struct Task {
    pub name: String,
    pub prompt: String,
    pub folder: PathBuf,
    /// Kill the core at this point of each run and start it again.
    pub kill_after: Option<KillAfter>,
}

/// `run.yml`. Unknown keys are refused: a misspelt one would run the task the plain way.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFile {
    kill_after: Option<KillAfter>,
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
        if !folder.join("task.md").is_file()
            || !only.is_empty() && !only.iter().any(|only| name.starts_with(only.as_str()))
        {
            continue;
        }
        let prompt = fs::read_to_string(folder.join("task.md"))?;
        let run = run_file(&folder).with_context(|| format!("task {name}"))?;
        found.push(Task {
            name,
            prompt: prompt.trim().to_owned(),
            folder,
            kill_after: run.kill_after,
        });
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    ensure!(!found.is_empty(), "no tasks found");
    Ok(found)
}

fn run_file(folder: &Path) -> Result<RunFile> {
    let path = folder.join("run.yml");
    if !path.exists() {
        return Ok(RunFile::default());
    }
    read_run_file(&fs::read_to_string(&path)?)
}

fn read_run_file(text: &str) -> Result<RunFile> {
    serde_saphyr::from_str(text).map_err(|error| anyhow::anyhow!("run.yml: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_yml_takes_a_count_or_a_range_and_refuses_the_rest() {
        let count = read_run_file("kill_after: 12\n").unwrap().kill_after;
        assert_eq!(count, Some(KillAfter::parse("12").unwrap()));
        let range = read_run_file("kill_after: 2..=40\n").unwrap().kill_after;
        assert_eq!(range, Some(KillAfter::parse("2..=40").unwrap()));
        for wrong in ["kill_after: 0\n", "kill_after: 40..=2\n", "kill-after: 3\n"] {
            assert!(read_run_file(wrong).is_err(), "{wrong:?} was taken");
        }
    }

    /// Every shipped task loads; the kill task asks for a kill and its original does not.
    #[test]
    fn the_shipped_tasks_load() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tasks");
        let tasks = load(&folder, &[]).unwrap();
        let named = |name: &str| tasks.iter().find(|task| task.name == name);
        for name in [
            "team-chain",
            "team-chain-kill",
            "team-fan-out",
            "team-small",
            "loop-flaky-test",
            "loop-narrow-test",
            "loop-ping-pong",
            "loop-missing-dependency",
        ] {
            assert!(named(name).is_some(), "{name} is missing");
        }
        assert!(named("team-chain-kill").unwrap().kill_after.is_some());
        assert!(named("team-chain").unwrap().kill_after.is_none());
    }
}
