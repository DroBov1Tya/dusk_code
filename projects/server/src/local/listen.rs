//! The core on its socket: accept a peer of this user only, at most 64 at once, run the
//! handshake and carry the client. The socket is bound by the holder of the lock and
//! removed when serving stops.

use std::{
    fs,
    io::ErrorKind,
    os::unix::fs::{FileTypeExt, PermissionsExt},
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
    handshake::{Admitted, Gate, MAX_HELLO},
    lines::Lines,
};
use crate::{
    auth::Limiter,
    stream::{MAX_FRAME, pump},
};

/// Socket clients at once; apart from the browsers' cap.
const MAX_CONNECTIONS: usize = 64;

/// How long `Serving::stop` waits for the listener to finish.
const STOP_WAIT: Duration = Duration::from_secs(5);

/// The core served on its socket; dropping it stops serving too.
pub struct Serving {
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl Serving {
    /// Stops accepting, removes the socket and ends every socket client's stream.
    pub async fn stop(mut self) {
        let _ = self.stop.send(true);
        if tokio::time::timeout(STOP_WAIT, &mut self.task)
            .await
            .is_err()
        {
            warn!("the socket listener did not stop in time");
        }
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
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
    let socket = files.socket_path()?.to_path_buf();
    remove_leftover(&socket)?;
    let listener = UnixListener::bind(&socket)
        .with_context(|| format!("cannot listen on {}", socket.display()))?;
    // The 0700 folder is what keeps others out; this only narrows the file as well.
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .context("cannot narrow the core socket")?;
    let gate = Arc::new(Gate {
        hub,
        key,
        owner,
        failures: Limiter::default(),
    });
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(accept(listener, socket, gate, stopped));
    Ok(Serving { stop, task })
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

async fn accept(
    listener: UnixListener,
    socket: PathBuf,
    gate: Arc<Gate>,
    mut stop: watch::Receiver<bool>,
) {
    let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
    let mut connections = JoinSet::new();
    let ours = euid();
    let ended = stop.clone();
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
                let Some(peer) = peer_uid(&stream) else {
                    warn!("a socket client of unknown user is dropped");
                    continue;
                };
                if !peer_allowed(peer, ours) {
                    warn!(uid = peer, "a socket client of another user is dropped");
                    continue;
                }
                let Ok(slot) = Arc::clone(&slots).try_acquire_owned() else {
                    debug!("too many socket clients; one is dropped");
                    continue;
                };
                connections.spawn(serve_one(stream, Arc::clone(&gate), ended.clone(), slot));
            }
        }
    }
    // New clients find nothing to connect to before the old ones are let go.
    drop(listener);
    if let Err(error) = fs::remove_file(&socket)
        && error.kind() != ErrorKind::NotFound
    {
        warn!(%error, "cannot remove the core socket");
    }
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
