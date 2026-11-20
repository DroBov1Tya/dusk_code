//! The core on its socket: accept a peer of this user only, at most 64 at once, run the
//! handshake and carry the client. The socket is bound by the holder of the lock and
//! removed when serving stops, but only while it is still the socket this core bound.

use std::{
    fs::{self, File},
    io::ErrorKind,
    os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail, ensure};
use dusk_core::client::Hub;
use dusk_protocol::client::OwnerKind;
use tokio::{
    net::{UnixListener, UnixStream},
    sync::{OwnedSemaphorePermit, Semaphore, watch},
    task::{JoinHandle, JoinSet},
};
use tracing::{debug, warn};

use super::{
    CoreLock, RunFiles,
    files::euid,
    handshake::{Admitted, Failures, Gate, Key, MAX_HELLO},
    lines::Lines,
};
use crate::stream::{MAX_FRAME, pump};

/// Socket clients at once; apart from the browsers' cap.
const MAX_CONNECTIONS: usize = 64;

/// How long `Serving::stop` waits for the listener to finish.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// The core served on its socket; dropping it stops serving too.
///
/// Stop or drop it before the `CoreLock` is released. It also keeps the lock itself until
/// its socket is gone, so a next core can never bind before this one unbinds.
pub struct Serving {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
    /// The socket as bound; `None` once it is removed.
    bound: Option<Bound>,
    /// The lock, shared: a `flock` lasts while any handle on it is open.
    _lock: File,
}

/// The socket file this core bound, by identity: a path alone may name another core's.
struct Bound {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Serving {
    /// Stops accepting, removes the socket and ends every socket client's stream.
    ///
    /// The clients' streams end at once, so stop the hub first and give its
    /// `CoreStopping` time to reach them when they should hear why.
    pub async fn stop(mut self) {
        self.unbind();
        let _ = self.stop.send(true);
        if tokio::time::timeout(STOP_WAIT, &mut self.task)
            .await
            .is_err()
        {
            warn!("the socket listener did not stop in time");
        }
    }

    /// Removes the socket, once, and only while the path still is the socket bound here.
    fn unbind(&mut self) {
        let Some(bound) = self.bound.take() else {
            return;
        };
        let ours = fs::symlink_metadata(&bound.path)
            .is_ok_and(|meta| meta.dev() == bound.dev && meta.ino() == bound.ino);
        if !ours {
            return;
        }
        if let Err(error) = fs::remove_file(&bound.path)
            && error.kind() != ErrorKind::NotFound
        {
            warn!(%error, "cannot remove the core socket");
        }
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        // Done here, not on the listener's task: that may run after a next core bound.
        self.unbind();
        let _ = self.stop.send(true);
    }
}

/// Serves `hub` on the socket of `files`. Only the holder of that core's lock may: a socket
/// left there by a crashed core is replaced, which no other process may do.
pub async fn serve(
    held: &CoreLock,
    hub: Arc<Hub>,
    files: RunFiles,
    key: [u8; 32],
    owner: OwnerKind,
) -> Result<Serving> {
    ensure!(
        held.path() == files.lock,
        "the socket of a core is bound only by the holder of its lock"
    );
    let lock = held.share()?;
    let socket = files.socket_path()?.to_path_buf();
    remove_leftover(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("cannot listen on {}", socket.display()))?;
    let meta = fs::symlink_metadata(&socket).context("cannot inspect the core socket")?;
    let bound = Bound {
        path: socket.clone(),
        dev: meta.dev(),
        ino: meta.ino(),
    };
    // The 0700 folder is what keeps others out; this only narrows the file as well.
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .context("cannot narrow the core socket")?;
    let gate = Arc::new(Gate {
        hub,
        key: Key(key),
        owner,
        failures: Failures::default(),
    });
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(accept(listener, gate, stopped));
    Ok(Serving {
        stop,
        task,
        bound: Some(bound),
        _lock: lock,
    })
}

/// Removes a socket a crashed core left; anything else at that path is not ours to remove.
fn remove_leftover(socket: &Path) -> Result<()> {
    match fs::symlink_metadata(socket) {
        Ok(meta) if meta.file_type().is_socket() => {
            fs::remove_file(socket).context("cannot remove the old core socket")
        }
        Ok(_) => bail!(
            "{} is not a socket; refusing to replace it",
            socket.display()
        ),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("cannot inspect the core socket"),
    }
}

async fn accept(listener: UnixListener, gate: Arc<Gate>, mut stop: watch::Receiver<bool>) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    let ours = euid();
    let ended = stop.clone();
    // Only root reaches a 0700 folder of another user: said once, not per connection.
    let mut stranger_logged = false;
    loop {
        tokio::select! {
            // The handle sent stop, or is gone: either way serving ends.
            () = async { let _ = stop.wait_for(|stop| *stop).await; } => break,
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _)) => stream,
                    Err(error) => {
                        warn!(%error, "cannot accept a socket client");
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        continue;
                    }
                };
                let peer = peer_uid(&stream);
                if !peer.is_some_and(|peer| peer_allowed(peer, ours)) {
                    if !stranger_logged {
                        warn!(uid = ?peer, "a socket client of another or unknown user is dropped");
                        stranger_logged = true;
                    }
                    continue;
                }
                let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                    // One try, never waited on: a client learns why instead of seeing EOF.
                    debug!("too many socket clients; one is turned away");
                    let _ = stream.try_write(gate.full_line().as_bytes());
                    continue;
                };
                connections.spawn(serve_one(stream, Arc::clone(&gate), ended.clone(), slot));
            }
        }
    }
    drop(listener);
    connections.shutdown().await;
}

fn peer_uid(stream: &UnixStream) -> Option<u32> {
    stream.peer_cred().ok().map(|cred| cred.uid())
}

/// Only this user's processes may talk to the core.
pub(super) fn peer_allowed(uid: u32, ours: u32) -> bool {
    uid == ours
}

async fn serve_one(
    stream: UnixStream,
    gate: Arc<Gate>,
    mut stop: watch::Receiver<bool>,
    _slot: OwnedSemaphorePermit,
) {
    let mut lines = Lines::new(stream, MAX_HELLO);
    let admitted = match gate.admit(&mut lines).await {
        Ok(Some(admitted)) => admitted,
        Ok(None) => return,
        Err(error) => {
            debug!("socket handshake ended: {error:#}");
            return;
        }
    };
    lines.set_max_line(MAX_FRAME);
    let Admitted { client, events } = admitted;
    if let Err(error) = pump(&mut lines, client, events, &mut stop, &|| true).await {
        debug!("socket stream ended: {error:#}");
    }
}

#[cfg(test)]
mod tests {
    use super::peer_allowed;

    #[test]
    fn a_peer_of_another_uid_is_dropped() {
        assert!(peer_allowed(501, 501));
        assert!(!peer_allowed(0, 501));
        assert!(!peer_allowed(65534, 501));
    }
}
