//! A started command: its process group, and tasks of their own reading its output.
//! Reading apart from waiting lets the process change hands while it runs, from the
//! tool that started it to the jobs manager, when the user sends it to the background.

use std::{collections::VecDeque, path::Path, sync::Arc, time::Duration};

use anyhow::{Context, Result, ensure};
use parking_lot::Mutex;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Child,
    sync::watch,
    task::JoinHandle,
};

use crate::platform::Confinement;

/// Each stream keeps its start and its end, where failures usually show.
const HEAD_BYTES: usize = 8 * 1024;
const TAIL_BYTES: usize = 24 * 1024;

pub struct Running {
    pub child: Child,
    pub group: ProcessGroup,
    pub stdout: Arc<Mutex<HeadTail>>,
    pub stderr: Arc<Mutex<HeadTail>>,
    readers: [JoinHandle<std::io::Result<()>>; 2],
    /// Set once the group is gone; readers then give a pipe a second more to drain.
    reaped: watch::Sender<bool>,
}

impl Running {
    /// `command` with `sh -c` in `cwd`, `env` added to its environment.
    pub fn start(
        command: &str,
        cwd: &Path,
        confinement: Option<&Confinement>,
        env: &[(&str, &Path)],
    ) -> Result<Self> {
        let mut shell = super::run_command::shell_command(command, cwd, confinement)?;
        for (name, value) in env {
            shell.env(name, value);
        }
        let mut child = shell.spawn().context("cannot start sh")?;
        let group = ProcessGroup(child.id());
        ensure!(group.0.is_some(), "command exited before it started");
        let stdout = child.stdout.take().context("stdout is not captured")?;
        let stderr = child.stderr.take().context("stderr is not captured")?;
        let (reaped, drained) = watch::channel(false);
        let (out, err) = (Arc::default(), Arc::default());
        let readers = [
            tokio::spawn(read(stdout, Arc::clone(&out), drained.clone())),
            tokio::spawn(read(stderr, Arc::clone(&err), drained)),
        ];
        Ok(Self {
            child,
            group,
            stdout: out,
            stderr: err,
            readers,
            reaped,
        })
    }

    /// Once the process ended or is to end: the group killed, the output read to its
    /// end. The streams as text, and a read failure, if any.
    pub async fn collect(mut self) -> (String, String, Option<String>) {
        // Background children would keep the pipes open; ending the group closes them.
        self.group.kill();
        let _ = self.child.wait().await;
        self.reaped.send_replace(true);
        let mut failure = None;
        for (name, reader) in ["stdout", "stderr"].into_iter().zip(self.readers) {
            match reader.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => failure = Some(format!("{name} read failed: {error}")),
                Err(_) => failure = Some(format!("{name} reader stopped")),
            }
        }
        let text = |kept: &Mutex<HeadTail>| kept.lock().text();
        (text(&self.stdout), text(&self.stderr), failure)
    }
}

/// Keeps reading past the cap so a chatty child never blocks on a full pipe. A child
/// that left the group can inherit a pipe; it cannot hold the reader past the reaping.
async fn read(
    mut stream: impl AsyncRead + Unpin,
    kept: Arc<Mutex<HeadTail>>,
    mut reaped: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let deadline = async move {
        // A dropped sender also counts: the command was dropped with its group.
        let _ = reaped.wait_for(|reaped| *reaped).await;
        tokio::time::sleep(Duration::from_secs(1)).await;
    };
    tokio::pin!(deadline);
    let mut buffer = [0u8; 8192];
    loop {
        let read = tokio::select! {
            read = stream.read(&mut buffer) => read?,
            _ = &mut deadline => return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "output pipe stayed open after the command ended",
            )),
        };
        if read == 0 {
            return Ok(());
        }
        kept.lock().push(&buffer[..read]);
    }
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

/// The first `HEAD_BYTES` and the last `TAIL_BYTES` of a stream, and how much fell
/// between them.
#[derive(Default)]
pub struct HeadTail {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    dropped: usize,
}

impl HeadTail {
    pub fn push(&mut self, mut bytes: &[u8]) {
        let room = HEAD_BYTES - self.head.len();
        let into_head = bytes.len().min(room);
        self.head.extend_from_slice(&bytes[..into_head]);
        bytes = &bytes[into_head..];
        self.tail.extend(bytes);
        let over = self.tail.len().saturating_sub(TAIL_BYTES);
        self.tail.drain(..over);
        self.dropped += over;
    }

    pub fn text(&self) -> String {
        let head = String::from_utf8_lossy(&self.head);
        let (front, back) = self.tail.as_slices();
        let tail = String::from_utf8_lossy(&[front, back].concat()).into_owned();
        match self.dropped {
            0 => format!("{head}{tail}"),
            dropped => format!("{head}\n[{dropped} bytes cut]\n{tail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_stream_keeps_its_start_and_end() {
        let mut kept = HeadTail::default();
        kept.push(b"start\n");
        kept.push(&vec![b'.'; HEAD_BYTES + TAIL_BYTES]);
        kept.push(b"\nerror: the end");
        let text = kept.text();
        assert!(text.starts_with("start\n"));
        assert!(text.ends_with("error: the end"));
        assert!(text.contains("bytes cut]"));
        assert!(text.len() < HEAD_BYTES + TAIL_BYTES + 64);
    }

    #[test]
    fn a_short_stream_is_whole() {
        let mut kept = HeadTail::default();
        kept.push(b"ok\n");
        assert_eq!(kept.text(), "ok\n");
    }
}
