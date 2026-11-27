//! One running terminal: the pseudo-terminal, the shell on it and three threads, since a
//! pseudo-terminal only reads and writes blocking. One carries what the shell prints to
//! the client, one what the client types to the shell, one waits for the shell to end.

use std::{
    io::{ErrorKind, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, SyncSender, TrySendError, sync_channel},
    },
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail};
use dusk_protocol::client::Event;
use portable_pty::{ChildKiller, MasterPty, PtySize, native_pty_system};
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

use super::{shell, tail::Tail};

/// The most bytes one `Event::Terminal` carries.
const CHUNK: usize = 64 * 1024;

/// Inputs that may wait for a shell that is not reading; more are refused, so a stuck
/// program cannot make the core keep what is typed at it without end.
const INPUT_QUEUE: usize = 64;

/// How long the last output may take to arrive after the shell ended, before the end is
/// told. A program left behind can hold the terminal open far longer than the shell.
const DRAIN: Duration = Duration::from_millis(250);

/// How long a shell told to leave has before it is killed.
const KILL_GRACE: Duration = Duration::from_secs(2);

pub struct Pty {
    master: Box<dyn MasterPty + Send>,
    input: SyncSender<Vec<u8>>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    pid: Option<u32>,
    exited: Arc<AtomicBool>,
}

/// Starts `shell` in `cwd` on a new pseudo-terminal. What it prints goes to `events` as
/// `Event::Terminal`, and its newest part to `tail`; when it ends, `ended` runs and
/// `Event::TerminalClosed` follows.
pub fn start(
    shell: &Path,
    cwd: &Path,
    size: PtySize,
    id: String,
    events: UnboundedSender<Event>,
    tail: Arc<parking_lot::Mutex<Tail>>,
    ended: impl FnOnce() + Send + 'static,
) -> Result<Pty> {
    let pair = native_pty_system()
        .openpty(size)
        .map_err(|error| anyhow!("cannot open a pseudo-terminal: {error:#}"))?;
    let reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| anyhow!("cannot open a pseudo-terminal: {error:#}"))?;
    let writer = pair
        .master
        .take_writer()
        .map_err(|error| anyhow!("cannot open a pseudo-terminal: {error:#}"))?;
    let mut child = pair
        .slave
        .spawn_command(shell::command(shell, cwd))
        .map_err(|error| anyhow!("cannot start the shell: {error:#}"))?;
    // Only the shell holds the other end now, so the reader sees it go.
    drop(pair.slave);

    let killer = child.clone_killer();
    let pid = child.process_id();
    let exited = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicBool::new(false));
    let (input, typed) = sync_channel(INPUT_QUEUE);
    let (drained, drain) = sync_channel::<()>(0);

    let threads = thread("terminal-output", {
        let (id, events, closed) = (id.clone(), events.clone(), Arc::clone(&closed));
        move || {
            read(reader, &id, &events, &tail, &closed);
            drop(drained);
        }
    })
    .and_then(|()| thread("terminal-input", move || write(writer, typed)));
    if let Err(error) = threads {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let wait = thread("terminal-wait", {
        let exited = Arc::clone(&exited);
        move || {
            let status = child.wait();
            exited.store(true, Ordering::Relaxed);
            // The reader hangs up when it has read the last of it, or the wait runs out.
            let _ = drain.recv_timeout(DRAIN);
            closed.store(true, Ordering::Relaxed);
            ended();
            info!("terminal {id} closed");
            let code = status
                .ok()
                .filter(|status| status.signal().is_none())
                .map(|status| status.exit_code() as i32);
            let _ = events.send(Event::TerminalClosed { id, code });
        }
    });
    let pty = Pty {
        master: pair.master,
        input,
        killer,
        pid,
        exited,
    };
    if let Err(error) = wait {
        // Nothing reaps the shell now; it is told to leave and killed all the same.
        pty.hang_up();
        return Err(error);
    }
    Ok(pty)
}

impl Pty {
    pub fn input(&self, data: Vec<u8>) -> Result<()> {
        match self.input.try_send(data) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => bail!("the terminal is not reading what is typed"),
            Err(TrySendError::Disconnected(_)) => bail!("the terminal has ended"),
        }
    }

    pub fn resize(&self, size: PtySize) -> Result<()> {
        self.master
            .resize(size)
            .map_err(|error| anyhow!("cannot resize the terminal: {error:#}"))
    }

    #[cfg(test)]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The folder the shell is in now, which `cd` moves; none where the system does not
    /// tell it.
    pub fn cwd(&self) -> Option<std::path::PathBuf> {
        self.pid.and_then(cwd_of)
    }

    /// A program other than the shell holds the terminal now, such as a dev server.
    pub fn busy(&self) -> bool {
        #[cfg(unix)]
        if let (Some(leader), Some(pid)) = (self.master.process_group_leader(), self.pid) {
            return leader as u32 != pid;
        }
        false
    }

    /// Ends the shell as a closed window does: it is told to leave, and killed if it
    /// has not left in `KILL_GRACE`. Needs the tokio runtime the core runs on.
    pub fn hang_up(mut self) {
        // The shell may be gone already; then there is nobody to tell.
        let _ = self.killer.kill();
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            let exited = Arc::clone(&self.exited);
            tokio::spawn(async move {
                tokio::time::sleep(KILL_GRACE).await;
                // The id is the shell's until the wait thread reaps it and sets `exited`.
                if !exited.load(Ordering::Relaxed) {
                    let _ = nix::sys::signal::killpg(
                        nix::unistd::Pid::from_raw(pid as i32),
                        nix::sys::signal::Signal::SIGKILL,
                    );
                }
            });
        }
    }
}

fn thread(name: &str, run: impl FnOnce() + Send + 'static) -> Result<()> {
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(run)
        .map(|_| ())
        .context("cannot start a thread for the terminal")
}

/// Hands on what the shell prints until it is gone, the client is, or the terminal closed.
fn read(
    mut reader: Box<dyn Read + Send>,
    id: &str,
    events: &UnboundedSender<Event>,
    tail: &parking_lot::Mutex<Tail>,
    closed: &AtomicBool,
) {
    let mut buffer = vec![0; CHUNK];
    loop {
        let read = match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(read) => read,
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            // How Linux says that the shell is gone.
            Err(_) => return,
        };
        tail.lock().push(&buffer[..read]);
        let event = Event::Terminal {
            id: id.to_owned(),
            data: buffer[..read].to_vec(),
        };
        // Leaving closes this end, which hangs up on a program left behind.
        if closed.load(Ordering::Relaxed) || events.send(event).is_err() {
            return;
        }
    }
}

fn write(mut writer: Box<dyn Write + Send>, typed: Receiver<Vec<u8>>) {
    for data in typed {
        if writer.write_all(&data).is_err() {
            return;
        }
    }
}

/// The working folder of the process `pid`.
#[cfg(target_os = "macos")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut info = std::mem::MaybeUninit::<libc::proc_vnodepathinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    // SAFETY: the buffer is a zeroed `proc_vnodepathinfo` of `size` bytes, which the call
    // fills at most; any byte pattern is a valid value of it.
    let filled = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if filled != size {
        return None;
    }
    // SAFETY: zeroed, then filled in full by the call above.
    let info = unsafe { info.assume_init() };
    let path: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .map(|byte| *byte as u8)
        .take_while(|byte| *byte != 0)
        .collect();
    (!path.is_empty()).then(|| std::ffi::OsStr::from_bytes(&path).into())
}

#[cfg(target_os = "linux")]
fn cwd_of(pid: u32) -> Option<std::path::PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn cwd_of(_pid: u32) -> Option<std::path::PathBuf> {
    None
}
