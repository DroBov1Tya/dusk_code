//! Runs a shell command in the working directory. The command gets its own process group,
//! so everything it started is killed when it ends or times out. Foreground commands
//! also stop with the turn; background commands belong to the runtime job manager.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::{process::Command, sync::watch};

use parking_lot::Mutex;

use crate::BoxFuture;
use crate::config::ApprovalPolicy;
use crate::platform::{self, Confinement};
use crate::tools::{
    schema,
    schema::parse_arguments,
    scope::{Place, ToolScope},
    tool::{Tool, ToolContext, ToolKind},
};
use crate::util::fs::printable;

use super::process::Running;

const DEFAULT_TIMEOUT_SECONDS: u64 = 120;
const MAX_TIMEOUT_SECONDS: u64 = 600;

#[derive(Default)]
pub struct RunCommand {
    jobs: Option<crate::runtime::jobs::JobOwner>,
    /// Where the last command ended, so a `cd` carries over to the next call. `None` is
    /// the project folder.
    dir: Mutex<Option<PathBuf>>,
}

impl RunCommand {
    pub fn with_jobs(jobs: crate::runtime::jobs::JobOwner) -> Self {
        Self {
            jobs: Some(jobs),
            ..Self::default()
        }
    }

    /// The folder the next command starts in. One that is gone or outside the tools'
    /// folders gives way to the project folder.
    fn start_dir(&self, scope: &ToolScope) -> PathBuf {
        let kept = self.dir.lock().clone();
        kept.filter(|dir| inside(scope, dir))
            .unwrap_or_else(|| scope.cwd().to_owned())
    }

    /// Keeps where the command ended; says so when that is outside and was not kept.
    fn keep_dir(&self, scope: &ToolScope, ended: Option<PathBuf>) -> Option<String> {
        let ended = ended?;
        if inside(scope, &ended) {
            *self.dir.lock() = (ended != scope.cwd()).then_some(ended);
            return None;
        }
        *self.dir.lock() = None;
        Some(format!(
            "[the shell ended in {}, outside the project; the next command starts in {}]",
            printable(&ended.to_string_lossy()),
            scope.cwd().display()
        ))
    }

    async fn start_job(
        &self,
        command: &str,
        timeout: u64,
        context: &ToolContext,
    ) -> Result<String> {
        let owner = self
            .jobs
            .as_ref()
            .context("background jobs require a running thread")?;
        let id = owner
            .manager
            .start(
                &owner.thread_id,
                command,
                &self.start_dir(&context.scope),
                confinement(context).await.as_ref(),
                timeout,
                context
                    .progress
                    .as_ref()
                    .map(|progress| progress.call_id.as_str()),
            )
            .await?;
        Ok(format!("background job started: job_id={id}"))
    }

    async fn run_here(
        &self,
        command: &str,
        timeout: u64,
        context: &ToolContext,
    ) -> Result<crate::tools::ToolResult> {
        let scope = &*context.scope;
        let confinement = confinement(context).await;
        let marker = scope.workspace().join(format!(
            ".dusk-cwd-{}-{}",
            std::process::id(),
            NEXT_MARKER.fetch_add(1, Ordering::Relaxed)
        ));
        // The trap runs on every exit, `exit` in the command included. It writes only
        // while the file is there, so a command sent to the background leaves none.
        tokio::fs::write(&marker, "")
            .await
            .context("cannot prepare the command")?;
        let script = format!(
            "trap '[ -f \"$DUSK_CWD_FILE\" ] && pwd -P >| \"$DUSK_CWD_FILE\" 2>/dev/null' EXIT\n{command}"
        );
        // The user may send this command to the background while it runs.
        let call_id = context
            .progress
            .as_ref()
            .map(|progress| progress.call_id.clone());
        let handed = match (&self.jobs, &call_id) {
            (Some(owner), Some(call_id)) => {
                Some(owner.manager.foreground(&owner.thread_id, call_id))
            }
            _ => None,
        };
        let ran = run(
            &script,
            &self.start_dir(scope),
            Some(&marker),
            scope,
            confinement.as_ref(),
            timeout,
            handed.as_ref().map(|handed| handed.request()),
        )
        .await;
        let ended = tokio::fs::read_to_string(&marker)
            .await
            .ok()
            .filter(|text| !text.trim().is_empty())
            .map(|text| PathBuf::from(text.trim_end_matches('\n')));
        let _ = tokio::fs::remove_file(&marker).await;
        drop(handed);
        let mut result = match ran? {
            Ran::Done(result) => result,
            Ran::Detached(running) => {
                let owner = self
                    .jobs
                    .as_ref()
                    .context("background jobs require a running thread")?;
                let id = owner
                    .manager
                    .adopt(
                        &owner.thread_id,
                        command,
                        call_id.as_deref(),
                        running,
                        MAX_TIMEOUT_SECONDS,
                    )
                    .await?;
                return Ok(crate::tools::ToolResult::text(format!(
                    "the user moved this command to the background: job_id={id}; its result \
                     joins this agent when it ends"
                )));
            }
        };
        if let Some(note) = self.keep_dir(scope, ended) {
            result.content.push_str(&format!("\n{note}"));
        }
        Ok(result)
    }
}

static NEXT_MARKER: AtomicU64 = AtomicU64::new(0);

fn inside(scope: &ToolScope, dir: &Path) -> bool {
    dir.is_dir()
        && scope
            .locate(&dir.to_string_lossy())
            .is_ok_and(|(_, place)| place == Place::Inside)
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunCommandArguments {
    /// Shell command line.
    command: String,
    /// What the command does, in a few words, shown to the user.
    #[allow(dead_code, reason = "the clients read it from the call")]
    description: Option<String>,
    /// Kill the command after this many seconds. Defaults to 120.
    #[schemars(range(min = 1, max = MAX_TIMEOUT_SECONDS))]
    timeout_seconds: Option<u64>,
    /// Return immediately and deliver completion to this agent automatically. Defaults to
    /// false.
    #[serde(default)]
    background: bool,
}

/// The arguments and the timeout, checked.
fn parse(arguments: Value) -> Result<(RunCommandArguments, u64)> {
    let arguments: RunCommandArguments = parse_arguments(arguments)?;
    ensure!(!arguments.command.trim().is_empty(), "command is empty");
    let timeout = arguments.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
    ensure!(
        (1..=MAX_TIMEOUT_SECONDS).contains(&timeout),
        "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
    );
    Ok((arguments, timeout))
}

impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn kind(&self) -> ToolKind {
        ToolKind::Console
    }

    fn description(&self, context: &ToolContext) -> String {
        let scope = &*context.scope;
        format!(
            "Run a command with sh -c. The first starts in {}; the folder a command ends in \
             carries over to the next, so `cd` once, not in every command. Stdin is empty. \
             Returns the exit code, stdout and stderr; a long stream keeps its first 8 KiB \
             and last 24 KiB. A non-zero exit code is a failed result. Put temporary and \
             downloaded files in {}. Change files here too: a quoted heredoc \
             (cat > file <<'EOF') for a new file; for an edit, a short script that replaces \
             one exact, unique piece and fails otherwise; then check with git diff.",
            scope.cwd().display(),
            scope.workspace().display()
        ) + if self.jobs.is_some() {
            " Set background=true to start a job and continue immediately. Returns job_id after spawn; the result joins this agent automatically at a safe boundary, even after this turn ends. Jobs survive turn cancellation; use jobs(action=cancel) to stop one. At most 8 jobs per thread and 64 per runtime; commands are limited to 16 KiB. Incomplete jobs are never restarted after application restart."
        } else {
            ""
        }
    }

    fn parameters(&self) -> Value {
        schema::of::<RunCommandArguments>()
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext) -> BoxFuture<'a, String> {
        Box::pin(async move { Ok(self.call_result(arguments, context).await?.content) })
    }

    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, crate::tools::ToolResult> {
        Box::pin(async move {
            let (arguments, timeout) = parse(arguments)?;
            context.approve_command(&arguments.command).await?;
            platform::login_path::load().await;
            if arguments.background {
                let started = self
                    .start_job(&arguments.command, timeout, &context)
                    .await?;
                return Ok(crate::tools::ToolResult::text(started));
            }
            self.run_here(&arguments.command, timeout, &context).await
        })
    }
}

/// How a foreground command ended: done, or handed on while it ran.
enum Ran {
    Done(crate::tools::ToolResult),
    Detached(Running),
}

async fn run(
    script: &str,
    dir: &Path,
    marker: Option<&Path>,
    scope: &ToolScope,
    confinement: Option<&Confinement>,
    timeout_seconds: u64,
    detach: Option<watch::Receiver<bool>>,
) -> Result<Ran> {
    let skills = super::skill_saves::snapshot(scope).await;
    let env: Vec<(&str, &Path)> = marker
        .map(|marker| ("DUSK_CWD_FILE", marker))
        .into_iter()
        .collect();
    let mut running = Running::start(script, dir, confinement, &env)?;
    let detached = async {
        let Some(mut detach) = detach else {
            return std::future::pending().await;
        };
        // A gone sender is no request.
        if detach.wait_for(|detach| *detach).await.is_err() {
            std::future::pending::<()>().await;
        }
    };
    let status = tokio::select! {
        status = running.child.wait() => status.context("cannot wait for the command")?,
        // Dropping the command kills its group.
        () = tokio::time::sleep(Duration::from_secs(timeout_seconds)) => {
            bail!("command timed out after {timeout_seconds} seconds and was killed")
        }
        () = detached => return Ok(Ran::Detached(running)),
    };
    let (stdout, stderr, failure) = running.collect().await;
    let code = match status.code() {
        Some(code) => code.to_string(),
        None => "none (terminated by a signal)".into(),
    };
    let mut output = format!("exit code: {code}");
    for (name, text) in [("stdout", stdout), ("stderr", stderr)] {
        if !text.is_empty() {
            output.push_str(&format!("\n{name}:\n{text}"));
        }
    }
    if let Some(failure) = failure {
        output.push_str(&format!("\n{failure}"));
    }
    if let Some(before) = skills
        && let Some(saved) = super::skill_saves::saved(scope, &before).await
    {
        // Last line, where clients look for it.
        output.push_str(&format!("\n{}", saved.stored()));
    }
    Ok(Ran::Done(crate::tools::ToolResult {
        content: output,
        is_error: !status.success(),
        exit_code: status.code(),
        artifacts: Vec::new(),
    }))
}

/// Most a command the user runs from the client may be, as for background jobs.
const MAX_USER_COMMAND_BYTES: usize = 16 * 1024;

/// Runs a command the user confirmed in the client, confined as the thread's tools are.
/// Its yes was the approval, so nothing is asked again.
pub async fn run_for_user(command: &str, context: &ToolContext) -> Result<String> {
    ensure!(!command.trim().is_empty(), "command is empty");
    ensure!(
        command.len() <= MAX_USER_COMMAND_BYTES,
        "command is longer than {MAX_USER_COMMAND_BYTES} bytes"
    );
    platform::login_path::load().await;
    let confinement = confinement(context).await;
    let scope = &*context.scope;
    let ran = run(
        command,
        scope.cwd(),
        None,
        scope,
        confinement.as_ref(),
        DEFAULT_TIMEOUT_SECONDS,
        None,
    )
    .await?;
    match ran {
        Ran::Done(result) => Ok(result.content),
        Ran::Detached(_) => bail!("a command the user runs is never handed on"),
    }
}

/// How the command is confined in sandbox mode, by the scope it runs in.
pub async fn confinement(context: &ToolContext) -> Option<Confinement> {
    let scope = &*context.scope;
    (context.approvals_policy().await == ApprovalPolicy::Sandbox)
        .then(|| Confinement::around(scope.cwd(), scope.workspace(), scope.protected()))
}

/// The command ready to start, with the user's shell PATH when it was read.
pub(super) fn shell_command(
    command: &str,
    cwd: &Path,
    confinement: Option<&Confinement>,
) -> Result<Command> {
    let mut shell = platform::shell(command, confinement)?;
    if let Some(path) = platform::login_path::get() {
        shell.env("PATH", path);
    }
    shell
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true);
    Ok(shell)
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::fs;

    use super::*;
    use crate::testing::{temp_dir, tool_context};

    fn scope(name: &str) -> ToolScope {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        ToolScope::new(&cwd, &workspace).unwrap()
    }

    async fn run_tool(scope: &ToolScope, arguments: Value) -> Result<String> {
        RunCommand::default()
            .call(arguments, tool_context(scope))
            .await
    }

    /// A `cd` carries to the next call inside the project; one out of it does not.
    #[tokio::test]
    async fn the_folder_carries_over_only_inside_the_project() {
        let scope = scope("shell-folder");
        fs::create_dir_all(scope.cwd().join("sub")).unwrap();
        let tool = RunCommand::default();
        let run = |command: &str| tool.call(json!({ "command": command }), tool_context(&scope));
        run("cd sub").await.unwrap();
        let here = run("pwd -P").await.unwrap();
        assert!(here.contains(&*scope.cwd().join("sub").to_string_lossy()));
        let left = run("cd /").await.unwrap();
        assert!(left.contains("outside the project"));
        let back = run("pwd -P").await.unwrap();
        assert!(back.contains(&*scope.cwd().to_string_lossy()));
        assert!(!back.contains("/sub"));
        assert_eq!(
            fs::read_dir(scope.workspace()).unwrap().count(),
            0,
            "no marker left"
        );
    }

    /// A skill saved by a command gets the notice clients look for; invalid text does not.
    #[tokio::test]
    async fn a_command_that_saves_a_skill_reports_it() {
        let skills = temp_dir("shell-skill-save").join("skills");
        fs::create_dir_all(&skills).unwrap();
        let scope = scope("shell-skill-save")
            .share(&skills)
            .with_skills(&skills);
        let folder = skills.canonicalize().unwrap().join("greet");
        let write = |text: &str| {
            json!({ "command": format!(
                "mkdir -p '{0}' && printf '%s' '{text}' > '{0}/SKILL.md'",
                folder.display()
            )})
        };
        let saved = run_tool(&scope, write("---\ndescription: Greets.\n---\nSay hi."))
            .await
            .unwrap();
        let notice = crate::catalog::skills::SkillSave::from_result(&saved).unwrap();
        assert_eq!(notice.name, "greet");
        let invalid = run_tool(&scope, write("no front matter")).await.unwrap();
        assert!(crate::catalog::skills::SkillSave::from_result(&invalid).is_none());
    }

    #[tokio::test]
    async fn foreground_exit_is_typed_even_if_output_claims_success() {
        let scope = scope("shell-typed-exit");
        let result = RunCommand::default()
            .call_result(
                json!({"command":"echo success; exit 7"}),
                tool_context(&scope),
            )
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(7));
        assert!(result.is_error);
        let result = RunCommand::default()
            .call_result(json!({"command":"exit 0"}), tool_context(&scope))
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn kills_on_timeout() {
        let scope = scope("shell_timeout");
        let started = std::time::Instant::now();
        let arguments = json!({ "command": "sleep 5", "timeout_seconds": 1 });
        let error = run_tool(&scope, arguments).await.unwrap_err();
        assert!(error.to_string().contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    /// Sent to the background mid-run, the command is handed on alive and its output
    /// keeps what came before and after.
    #[tokio::test]
    async fn a_command_handed_on_keeps_running() {
        let scope = scope("shell_handed_on");
        let (switch, request) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            switch.send_replace(true);
            // Kept alive until the command was handed on.
            tokio::time::sleep(Duration::from_secs(2)).await;
        });
        let started = std::time::Instant::now();
        let script = "echo before; sleep 0.4; echo after";
        let ran = run(script, scope.cwd(), None, &scope, None, 10, Some(request))
            .await
            .unwrap();
        let Ran::Detached(mut running) = ran else {
            panic!("the command finished in the foreground");
        };
        assert!(
            started.elapsed() < Duration::from_millis(350),
            "handed on at once"
        );
        running.child.wait().await.unwrap();
        let (stdout, _, failure) = running.collect().await;
        assert_eq!(failure, None);
        assert!(
            stdout.contains("before") && stdout.contains("after"),
            "{stdout}"
        );
    }

    #[tokio::test]
    async fn background_processes_do_not_outlive_the_command() {
        let scope = scope("shell_background");
        let started = std::time::Instant::now();
        let ran = run(
            "sleep 30 & echo started",
            scope.cwd(),
            None,
            &scope,
            None,
            10,
            None,
        )
        .await
        .unwrap();
        let Ran::Done(output) = ran else {
            panic!("nothing asked for the background");
        };
        assert!(output.content.contains("started"));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited for the background job"
        );
    }
}
