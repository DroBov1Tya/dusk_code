//! Runs a shell command in the working directory. The command gets its own process group,
//! so everything it started is killed when it ends or times out. Foreground commands
//! also stop with the turn; background commands belong to the runtime job manager.

use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
};

use crate::BoxFuture;
use crate::config::ApprovalPolicy;
use crate::platform::{self, Confinement};
use crate::tools::{
    schema,
    schema::parse_arguments,
    scope::ToolScope,
    tool::{Tool, ToolContext, ToolKind},
};

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 600;
/// Each stream keeps its start and its end, where failures usually show.
const HEAD_BYTES: usize = 8 * 1024;
const TAIL_BYTES: usize = 24 * 1024;

#[derive(Default)]
pub struct RunCommand {
    jobs: Option<crate::runtime::jobs::JobOwner>,
}

impl RunCommand {
    pub fn with_jobs(jobs: crate::runtime::jobs::JobOwner) -> Self {
        Self { jobs: Some(jobs) }
    }
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RunCommandArguments {
    /// Shell command line.
    command: String,
    /// Kill the command after this many seconds. Defaults to 60.
    #[schemars(range(min = 1, max = MAX_TIMEOUT_SECONDS))]
    timeout_seconds: Option<u64>,
    /// Return immediately and deliver completion to this agent automatically. Defaults to
    /// false.
    #[serde(default)]
    background: bool,
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
            "Run a command with sh -c in {}. Stdin is empty. Returns the exit code, stdout \
             and stderr; a long stream keeps its first 8 KiB and last 24 KiB. A non-zero exit code is a failed result. Put \
             temporary and downloaded files in {}. Change files here too: a quoted heredoc \
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
        Box::pin(async move {
            let scope = &*context.scope;
            let arguments: RunCommandArguments = parse_arguments(arguments)?;
            ensure!(!arguments.command.trim().is_empty(), "command is empty");
            let timeout = arguments.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
            ensure!(
                (1..=MAX_TIMEOUT_SECONDS).contains(&timeout),
                "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
            );
            context.approve_command(&arguments.command).await?;
            if arguments.background {
                let owner = self
                    .jobs
                    .as_ref()
                    .context("background jobs require a running thread")?;
                let id = owner
                    .manager
                    .start(
                        &owner.thread_id,
                        &arguments.command,
                        scope.cwd(),
                        confinement(&context).await.as_ref(),
                        timeout,
                        context
                            .progress
                            .as_ref()
                            .map(|progress| progress.call_id.as_str()),
                    )
                    .await?;
                return Ok(format!("background job started: job_id={id}"));
            }
            let confinement = confinement(&context).await;
            Ok(
                run(&arguments.command, scope, confinement.as_ref(), timeout)
                    .await?
                    .content,
            )
        })
    }
    fn call_result<'a>(
        &'a self,
        arguments: Value,
        context: ToolContext,
    ) -> BoxFuture<'a, crate::tools::ToolResult> {
        Box::pin(async move {
            let parsed: RunCommandArguments = parse_arguments(arguments.clone())?;
            ensure!(!parsed.command.trim().is_empty(), "command is empty");
            let timeout = parsed.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
            ensure!(
                (1..=MAX_TIMEOUT_SECONDS).contains(&timeout),
                "invalid timeout_seconds"
            );
            if parsed.background {
                return Ok(crate::tools::ToolResult::text(
                    self.call(arguments, context).await?,
                ));
            }
            context.approve_command(&parsed.command).await?;
            let confinement = confinement(&context).await;
            run(
                &parsed.command,
                &context.scope,
                confinement.as_ref(),
                timeout,
            )
            .await
        })
    }
}

async fn run(
    command: &str,
    scope: &ToolScope,
    confinement: Option<&Confinement>,
    timeout_seconds: u64,
) -> Result<crate::tools::ToolResult> {
    let skills = super::skill_saves::snapshot(scope).await;
    let mut child = spawn(command, scope.cwd(), confinement)?;
    let mut group = ProcessGroup(child.id());
    ensure!(group.0.is_some(), "command exited before it started");
    let stdout = child.stdout.take().context("stdout is not captured")?;
    let stderr = child.stderr.take().context("stderr is not captured")?;
    // Background processes would keep the pipes open; ending the group closes them.
    let exit = async {
        let status = child.wait().await;
        group.kill();
        status
    };
    let finished = async { tokio::try_join!(read_capped(stdout), read_capped(stderr), exit) };
    let outcome = tokio::time::timeout(Duration::from_secs(timeout_seconds), finished).await;
    let Ok(result) = outcome else {
        bail!("command timed out after {timeout_seconds} seconds and was killed");
    };
    let (stdout, stderr, status) = result.context("cannot read command output")?;
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
    if let Some(before) = skills
        && let Some(saved) = super::skill_saves::saved(scope, &before).await
    {
        // Last line, where clients look for it.
        output.push_str(&format!("\n{}", saved.stored()));
    }
    Ok(crate::tools::ToolResult {
        content: output,
        is_error: !status.success(),
        exit_code: status.code(),
        artifacts: Vec::new(),
    })
}

/// How the command is confined in sandbox mode, by the scope it runs in.
pub async fn confinement(context: &ToolContext) -> Option<Confinement> {
    let scope = &*context.scope;
    (context.approvals_policy().await == ApprovalPolicy::Sandbox)
        .then(|| Confinement::around(scope.cwd(), scope.workspace(), scope.protected()))
}

pub fn spawn(command: &str, cwd: &Path, confinement: Option<&Confinement>) -> Result<Child> {
    platform::shell(command, confinement)?
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("cannot start sh")
}

/// Kills the whole group when dropped, so a cancelled or timed-out command leaves nothing
/// behind. Kills at most once: a later kill could hit a new group that reused the ID.
pub struct ProcessGroup(pub Option<u32>);

impl ProcessGroup {
    pub fn kill(&mut self) {
        let Some(id) = self.0.take() else {
            return;
        };
        #[cfg(unix)]
        if let Ok(id) = i32::try_from(id) {
            // The group may already be gone; nothing is left to do then.
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(id),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
    }
}

impl Drop for ProcessGroup {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Keeps reading past the cap so a chatty child never blocks on a full pipe.
async fn read_capped(mut stream: impl AsyncRead + Unpin) -> std::io::Result<String> {
    let mut kept = HeadTail::default();
    let mut buffer = [0u8; 8192];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        kept.push(&buffer[..read]);
    }
    Ok(kept.finish())
}

/// The first `HEAD_BYTES` and the last `TAIL_BYTES` of a stream, and how much fell
/// between them.
#[derive(Default)]
struct HeadTail {
    head: Vec<u8>,
    tail: std::collections::VecDeque<u8>,
    dropped: usize,
}

impl HeadTail {
    fn push(&mut self, mut bytes: &[u8]) {
        let room = HEAD_BYTES - self.head.len();
        let into_head = bytes.len().min(room);
        self.head.extend_from_slice(&bytes[..into_head]);
        bytes = &bytes[into_head..];
        self.tail.extend(bytes);
        let over = self.tail.len().saturating_sub(TAIL_BYTES);
        self.tail.drain(..over);
        self.dropped += over;
    }

    fn finish(self) -> String {
        let head = String::from_utf8_lossy(&self.head).into_owned();
        let tail = String::from_utf8_lossy(&Vec::from(self.tail)).into_owned();
        match self.dropped {
            0 => head + &tail,
            dropped => format!("{head}\n[{dropped} bytes cut]\n{tail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::fs;

    use super::*;
    use crate::testing::{temp_dir, tool_context};

    #[test]
    fn a_long_stream_keeps_its_start_and_end() {
        let mut kept = HeadTail::default();
        kept.push(b"start\n");
        kept.push(&vec![b'.'; HEAD_BYTES + TAIL_BYTES]);
        kept.push(b"\nerror: the end");
        let text = kept.finish();
        assert!(text.starts_with("start\n"));
        assert!(text.ends_with("error: the end"));
        assert!(text.contains("bytes cut]"));
        assert!(text.len() < HEAD_BYTES + TAIL_BYTES + 64);
    }

    #[test]
    fn a_short_stream_is_whole() {
        let mut kept = HeadTail::default();
        kept.push(b"ok\n");
        assert_eq!(kept.finish(), "ok\n");
    }

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

    #[tokio::test]
    async fn background_processes_do_not_outlive_the_command() {
        let scope = scope("shell_background");
        let started = std::time::Instant::now();
        let output = run("sleep 30 & echo started", &scope, None, 10)
            .await
            .unwrap();
        assert!(output.content.contains("started"));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited for the background job"
        );
    }
}
