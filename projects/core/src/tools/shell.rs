//! Runs a shell command in the working directory. The command gets its own process group,
//! so everything it started is killed when it ends, times out or the turn is cancelled.

use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

use super::{
    builtin::parse_arguments,
    scope::ToolScope,
    tool::{Tool, ToolContext},
};
use crate::BoxFuture;

const DEFAULT_TIMEOUT_SECONDS: u64 = 60;
const MAX_TIMEOUT_SECONDS: u64 = 600;
const MAX_STREAM_BYTES: usize = 32 * 1024;

pub struct RunCommand;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunCommandArguments {
    command: String,
    timeout_seconds: Option<u64>,
}

impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }

    fn description(&self, context: &ToolContext<'_>) -> String {
        let scope = context.scope;
        format!(
            "Run a command with sh -c in {}. Stdin is empty. Returns the exit code, stdout \
             and stderr, each cut at 32 KiB. A non-zero exit code is a normal result. Put \
             temporary and downloaded files in {}.",
            scope.cwd().display(),
            scope.workspace().display()
        )
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "Shell command line." },
                "timeout_seconds": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_TIMEOUT_SECONDS,
                    "description": "Kill the command after this many seconds. Defaults to 60."
                }
            },
            "required": ["command"],
            "additionalProperties": false
        })
    }

    fn call<'a>(&'a self, arguments: Value, context: ToolContext<'a>) -> BoxFuture<'a, String> {
        let scope = context.scope;
        Box::pin(async move {
            let arguments: RunCommandArguments = parse_arguments(arguments)?;
            ensure!(!arguments.command.trim().is_empty(), "command is empty");
            let timeout = arguments.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT_SECONDS);
            ensure!(
                (1..=MAX_TIMEOUT_SECONDS).contains(&timeout),
                "timeout_seconds must be between 1 and {MAX_TIMEOUT_SECONDS}"
            );
            run(&arguments.command, scope, timeout).await
        })
    }
}

async fn run(command: &str, scope: &ToolScope, timeout_seconds: u64) -> Result<String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(scope.cwd())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("cannot start sh")?;
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
    Ok(output)
}

/// Kills the whole group when dropped, so a cancelled or timed-out command leaves nothing
/// behind. Kills at most once: a later kill could hit a new group that reused the ID.
struct ProcessGroup(Option<u32>);

impl ProcessGroup {
    fn kill(&mut self) {
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
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let room = MAX_STREAM_BYTES - kept.len();
        kept.extend_from_slice(&buffer[..read.min(room)]);
        truncated |= read > room;
    }
    let mut text = String::from_utf8_lossy(&kept).into_owned();
    if truncated {
        text.push_str("\n[output truncated]");
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
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
        RunCommand.call(arguments, tool_context(scope)).await
    }

    #[tokio::test]
    async fn reports_exit_code_and_both_streams() {
        let scope = scope("shell_streams");
        let command = "echo out; echo err >&2; exit 3";
        let output = run_tool(&scope, json!({ "command": command }))
            .await
            .unwrap();
        assert_eq!(output, "exit code: 3\nstdout:\nout\n\nstderr:\nerr\n");
    }

    #[tokio::test]
    async fn omits_empty_sections() {
        let scope = scope("shell_empty");
        let output = run_tool(&scope, json!({ "command": "true" }))
            .await
            .unwrap();
        assert_eq!(output, "exit code: 0");
    }

    #[tokio::test]
    async fn runs_in_the_working_directory() {
        let scope = scope("shell_pwd");
        let output = run_tool(&scope, json!({ "command": "pwd -P" }))
            .await
            .unwrap();
        let expected = format!("exit code: 0\nstdout:\n{}\n", scope.cwd().display());
        assert_eq!(output, expected);
    }

    #[tokio::test]
    async fn caps_stream_output() {
        let scope = scope("shell_cap");
        let command = "head -c 100000 /dev/zero | tr '\\0' a";
        let output = run_tool(&scope, json!({ "command": command }))
            .await
            .unwrap();
        assert!(output.ends_with("\n[output truncated]"), "{output}");
        assert!(output.len() < MAX_STREAM_BYTES + 100);
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
    async fn rejects_bad_timeouts() {
        let scope = scope("shell_bad_timeout");
        for timeout in [0, MAX_TIMEOUT_SECONDS + 1] {
            let arguments = json!({ "command": "true", "timeout_seconds": timeout });
            assert!(run_tool(&scope, arguments).await.is_err());
        }
    }

    #[tokio::test]
    async fn background_processes_do_not_outlive_the_command() {
        let scope = scope("shell_background");
        let started = std::time::Instant::now();
        let output = run("sleep 30 & echo started", &scope, 10).await.unwrap();
        assert!(output.contains("started"));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "waited for the background job"
        );
    }
}
