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
use crate::{plugins::ui::process::read_line, tools::console::process::ProcessGroup};

/// From the start of the program to its result; then it is killed.
pub(super) const LIMIT: Duration = Duration::from_secs(45);
const MAX_LOG: usize = 500;
const MAX_LOGS: usize = 20;
const MAX_STDERR: u64 = 4096;

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
                // Its own group, so whatever it starts is killed with it.
                .process_group(0)
                .kill_on_drop(true);
            Ok(command.spawn()?)
        })
        .map_err(|error| failed(format!("cannot start {plugin}: {error:#}")))?;
    let mut group = ProcessGroup(child.id());
    let answer = tokio::time::timeout(LIMIT, talk(plugin, call, &mut child)).await;
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
async fn talk(plugin: &str, call: &Call, child: &mut Child) -> Result<Answer, ContractError> {
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
            let text = untrusted::plain(&String::from_utf8_lossy(&kept), MAX_STDERR as usize);
            debug!("contract program of {plugin_name}, stderr: {text}");
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
    let answer = read_answer(&mut BufReader::new(stdout), &mut logs).await;
    for text in logs {
        debug!("contract program of {plugin}: {text}");
    }
    answer
}

/// Reads up to the first `result`; `logs` gets the first `MAX_LOGS` log lines, plain
/// and capped. Anything else before the result fails the call.
pub(super) async fn read_answer(
    reader: &mut (impl AsyncBufRead + Unpin),
    logs: &mut Vec<String>,
) -> Result<Answer, ContractError> {
    let mut line = Vec::new();
    loop {
        match read_line(reader, &mut line).await {
            Ok(true) => {}
            Ok(false) => return Err(failed("it ended without a result".into())),
            Err(why) => return Err(failed(format!("its output {why}"))),
        }
        let taken = std::mem::take(&mut line);
        match serde_json::from_slice::<FromProgram>(&taken) {
            Ok(FromProgram::Result(answer)) => return Ok(answer),
            Ok(FromProgram::Log { text }) => {
                if logs.len() < MAX_LOGS {
                    logs.push(untrusted::line(&text, MAX_LOG));
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
