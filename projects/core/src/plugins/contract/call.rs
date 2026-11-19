//! One call: the confinement built for this call's project, the program started in it,
//! one `call` line written, lines read with a cap until the first `result`, and the
//! whole process group killed after it or at the time limit.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use dusk_protocol::plugin::contract::{Answer, Call, ContractError, ErrorKind, FromProgram};
use tokio::{
    io::{AsyncBufRead, AsyncReadExt, AsyncWriteExt, BufReader},
    process::Child,
};
use tracing::debug;

use super::{Served, failed, untrusted};
use crate::{
    plugins::lines::{Line, Lines},
    tools::console::process::ProcessGroup,
};

/// From the start of the program to its result; then it is killed.
pub(super) const LIMIT: Duration = Duration::from_secs(45);
const MAX_LOG: usize = 500;
const MAX_LOGS: usize = 20;
const MAX_STDERR: u64 = 4096;
/// A line larger than this fails: parsed, a 4 MiB line of empty items could grow
/// to some hundred MB. A real answer is a few dozen KiB.
const MAX_RESULT: usize = 512 * 1024;
/// How long the output is still read after the program exited, for what it wrote last
/// and for a descendant that held the pipe.
const GRACE: Duration = Duration::from_secs(1);

/// Runs one call of `served` about the project in `folder` (canonical).
pub(super) async fn run(
    served: &Served,
    call: &Call,
    folder: &Path,
    others: &[PathBuf],
) -> Result<Answer, ContractError> {
    let plugin = &served.plugin;
    // Waits for a slot before anything starts, so a queued call holds no process.
    let _slot = served
        .slots
        .acquire()
        .await
        .map_err(|_| failed("its calls were closed".into()))?;
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| failed("HOME is not set".into()))?;
    // Built anew every call: it asks git, outside any sandbox, where the git dir is.
    let isolation = served.isolation.clone();
    let (project, others) = (folder.to_owned(), others.to_vec());
    let isolation =
        tokio::task::spawn_blocking(move || isolation.for_call(&home, &project, &others))
            .await
            .map_err(|error| failed(format!("cannot confine the call: {error}")))?
            .map_err(|error| failed(format!("cannot confine the call: {error:#}")))?;
    let mut child = isolation
        .command(&served.command)
        .and_then(|mut command| {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            // Its own session, so its own group, killed with whatever it starts, and no
            // controlling terminal of the core's to push keystrokes into.
            // SAFETY: one async-signal-safe system call in the forked child.
            unsafe {
                command.pre_exec(|| match libc::setsid() {
                    -1 => Err(std::io::Error::last_os_error()),
                    _ => Ok(()),
                });
            }
            Ok(command.spawn()?)
        })
        .map_err(|error| failed(format!("cannot start {plugin}: {error:#}")))?;
    let mut group = ProcessGroup(child.id());
    let answer = tokio::time::timeout(LIMIT, talk(plugin, call, &mut child, &mut group)).await;
    // Killed while it is not yet reaped, so its group cannot be another's.
    group.kill();
    let _ = child.kill().await;
    answer.unwrap_or_else(|_| {
        Err(ContractError {
            kind: ErrorKind::Unavailable,
            message: format!("{plugin} gave no result within {} s", LIMIT.as_secs()),
            hint: String::new(),
        })
    })
}

/// Writes the call, then reads its answer.
async fn talk(
    plugin: &str,
    call: &Call,
    child: &mut Child,
    group: &mut ProcessGroup,
) -> Result<Answer, ContractError> {
    let (Some(mut stdin), Some(stdout), Some(stderr)) =
        (child.stdin.take(), child.stdout.take(), child.stderr.take())
    else {
        return Err(failed("no pipes to the program".into()));
    };
    let plugin_name = plugin.to_owned();
    tokio::spawn(tokio::time::timeout(LIMIT, async move {
        let mut kept = Vec::new();
        let mut stderr = stderr;
        let _ = (&mut stderr).take(MAX_STDERR).read_to_end(&mut kept).await;
        // The rest is read and dropped, so the program never blocks on a full pipe.
        let _ = tokio::io::copy(&mut stderr, &mut tokio::io::sink()).await;
        if !kept.is_empty() {
            // Line by line, so the program cannot write a line that reads as the core's.
            let text = String::from_utf8_lossy(&kept);
            let lines = text.lines().filter(|line| !line.trim().is_empty());
            for text in lines.take(MAX_LOGS) {
                let text = untrusted::line(text, MAX_LOG);
                debug!("contract program of {plugin_name}, stderr: {text}");
            }
        }
    }));
    let mut line = serde_json::to_string(call)
        .map_err(|error| failed(format!("cannot write the call: {error}")))?;
    line.push('\n');
    // A program that answers without reading closes its end; its answer still counts.
    if stdin.write_all(line.as_bytes()).await.is_ok() {
        let _ = stdin.shutdown().await;
    }
    drop(stdin);
    let mut logs = Vec::new();
    let mut reader = BufReader::new(stdout);
    let read = read_answer(&mut reader, plugin, &mut logs);
    tokio::pin!(read);
    tokio::select! {
        answer = &mut read => answer,
        // Gone without a result: whatever it left running holds the pipe, so the group
        // is killed (the leader is a zombie still, so its group id is pinned) and the
        // rest is read only for a short while. `run` reaps it afterwards.
        () = exited(child.id()) => {
            group.kill();
            match tokio::time::timeout(GRACE, &mut read).await {
                Ok(answer) => answer,
                Err(_) => Err(failed("it ended without a result".into())),
            }
        }
    }
}

/// Resolves once the process has exited, without reaping it.
async fn exited(pid: Option<u32>) {
    let Some(pid) = pid else {
        return std::future::pending().await;
    };
    let mut tick = tokio::time::interval(Duration::from_millis(25));
    loop {
        tick.tick().await;
        if has_exited(pid) {
            return;
        }
    }
}

/// `waitid` with `WNOWAIT`: tells of an exit and leaves the zombie to be reaped later.
fn has_exited(pid: u32) -> bool {
    // SAFETY: `info` is plain data the call fills in; the pid is our own child's.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let done = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        #[cfg(target_os = "linux")]
        let who = info.si_pid();
        #[cfg(not(target_os = "linux"))]
        let who = info.si_pid;
        done == 0 && who != 0
    }
}

/// Reads up to the first `result`; `logs` gets the first `MAX_LOGS` log lines, plain
/// and capped, each also written to the debug log as it arrives, so a call that times
/// out keeps them. Anything else before the result fails the call.
pub(super) async fn read_answer(
    reader: &mut (impl AsyncBufRead + Unpin),
    plugin: &str,
    logs: &mut Vec<String>,
) -> Result<Answer, ContractError> {
    // Cancel-safe: `talk` races this read against the program's exit.
    let mut lines = Lines::new(MAX_RESULT);
    loop {
        let taken = match lines.next(reader).await {
            Ok(Line::Full(line)) => line,
            Ok(Line::End) => return Err(failed("it ended without a result".into())),
            Ok(Line::TooLong) => return Err(failed("its line is too large".into())),
            Err(error) => return Err(failed(format!("its output could not be read: {error}"))),
        };
        match serde_json::from_slice::<FromProgram>(&taken) {
            Ok(FromProgram::Result(answer)) => return Ok(answer),
            Ok(FromProgram::Log { text }) => {
                if logs.len() < MAX_LOGS {
                    let text = untrusted::line(&text, MAX_LOG);
                    debug!("contract program of {plugin}: {text}");
                    logs.push(text);
                }
            }
            Err(error) => {
                return Err(failed(format!(
                    "it wrote a line that is not a log or a result: {error}"
                )));
            }
        }
    }
}
