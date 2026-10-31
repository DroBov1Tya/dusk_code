//! A task's project in a folder of its own, under git so a check can see what changed, with
//! its check kept out of the agent's reach.

use std::{
    env, fs,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use uuid::Uuid;

use crate::task::Task;

/// What a setup, a solution or a check may take.
const SCRIPT_LIMIT: Duration = Duration::from_secs(120);

/// A private folder that goes away with the value: it can hold a copy of the API key.
pub struct TempHome(pub PathBuf);

impl TempHome {
    pub fn new(name: &str) -> Result<Self> {
        let path = env::temp_dir().join(format!("{name}-{}", Uuid::now_v7()));
        fs::create_dir(&path).context("cannot make a temporary folder")?;
        private(&path, 0o700)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for TempHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub fn private(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).context("cannot set permissions")
}

/// Runs a script of the task. One that hangs fails, along with all it started: an agent's
/// code may well hang, and a run must not stop for it.
fn run(script: &Path, cwd: &Path, answer: &Path) -> Result<bool> {
    let mut child = Command::new("sh")
        .arg(script)
        .current_dir(cwd)
        .env("EVAL_ANSWER", answer)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .with_context(|| format!("cannot run {}", script.display()))?;
    let started = Instant::now();
    while started.elapsed() < SCRIPT_LIMIT {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        thread::sleep(Duration::from_millis(50));
    }
    let group = format!("-{}", child.id());
    let _ = Command::new("kill").args(["-9", "--", &group]).status();
    child.wait()?;
    Ok(false)
}

fn git(cwd: &Path, arguments: &[&str]) -> Result<()> {
    let status = Command::new("git")
        .args(["-c", "user.name=eval", "-c", "user.email=eval@localhost"])
        .args(arguments)
        .current_dir(cwd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("cannot run git")?;
    ensure!(status.success(), "git {arguments:?} failed");
    Ok(())
}

/// The task's project at `into`, as one commit.
pub fn prepare(task: &Task, into: &Path) -> Result<()> {
    let status = Command::new("cp")
        .args(["-R"])
        .arg(task.folder.join("fixture"))
        .arg(into)
        .status()
        .context("cannot copy the fixture")?;
    ensure!(status.success(), "cannot copy the fixture of {}", task.name);
    let setup = task.folder.join("setup.sh").canonicalize();
    if let Ok(setup) = setup {
        ensure!(run(&setup, into, Path::new("/dev/null"))?, "setup failed");
    }
    git(into, &["init", "-q"])?;
    git(into, &["add", "-A"])?;
    git(into, &["commit", "-q", "-m", "start"])
}

/// Whether the work in `cwd` passes the task's check, given the agent's last message.
pub fn check(task: &Task, cwd: &Path, answer: &str, home: &Path) -> Result<bool> {
    let file = home.join(format!("{}-answer.txt", Uuid::now_v7()));
    fs::write(&file, answer)?;
    let script = task.folder.join("check.sh").canonicalize()?;
    let passed = run(&script, cwd, &file);
    let _ = fs::remove_file(&file);
    passed
}

/// A check that passes on untouched work, or fails on the known solution, measures nothing.
pub fn selfcheck(tasks: &[Task]) -> Result<()> {
    let mut wrong = 0;
    for task in tasks {
        let home = TempHome::new("dusk-eval-check")?;
        let untouched = home.0.join("untouched");
        prepare(task, &untouched)?;
        let idle = check(task, &untouched, "", &home.0)?;
        let solved = home.0.join("solved");
        prepare(task, &solved)?;
        let solution = task.folder.join("solution.sh").canonicalize()?;
        let applied = run(&solution, &solved, Path::new("/dev/null"))?;
        let answer = fs::read_to_string(task.folder.join("answer.txt")).unwrap_or_default();
        let passes = applied && check(task, &solved, &answer, &home.0)?;
        let good = !idle && passes;
        println!(
            "{} {:<24} untouched {}  solved {}",
            if good { "ok " } else { "BAD" },
            task.name,
            if idle { "passes" } else { "fails" },
            if passes { "passes" } else { "fails" },
        );
        wrong += usize::from(!good);
    }
    if wrong > 0 {
        bail!("{wrong} of {} checks are wrong", tasks.len());
    }
    println!("all {} checks tell work from no work", tasks.len());
    Ok(())
}
