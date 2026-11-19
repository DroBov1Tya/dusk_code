//! Commands the user runs from a chat's code block: answered beside the loop, since a
//! command may take minutes and the client keeps asking other things meanwhile, and
//! stopped on request. A run is known by its client, its thread and the client's number
//! for it, so one client cannot stop another's.

use std::{collections::HashMap, sync::Arc};

use dusk_protocol::client::Response;
use parking_lot::Mutex;
use tokio::sync::oneshot;

use super::{ClientId, Server};
use crate::client::Area;

type Key = (ClientId, String, u64);

/// The runs under way, each with the way to stop it.
#[derive(Clone, Default)]
pub struct UserRuns(Arc<Mutex<HashMap<Key, oneshot::Sender<()>>>>);

impl UserRuns {
    /// Stops every run of a client that went away: nobody is left to see the output.
    pub fn stop_client(&self, client: ClientId) {
        self.0.lock().retain(|(owner, _, _), _| *owner != client);
    }

    /// True when the run was under way; it answers its own request with the stop.
    fn stop(&self, key: &Key) -> bool {
        match self.0.lock().remove(key) {
            Some(stop) => {
                let _ = stop.send(());
                true
            }
            None => false,
        }
    }
}

impl Server {
    pub(super) fn run_command(
        &self,
        client: ClientId,
        thread_id: String,
        command: String,
        run: u64,
        reply: oneshot::Sender<Response>,
    ) {
        let key = (client, thread_id.clone(), run);
        let (stop, stopped) = oneshot::channel();
        let runs = self.user_runs.clone();
        if runs.0.lock().insert(key.clone(), stop).is_some() {
            tracing::debug!("run {run} of client {client} started again; the first is stopped");
        }
        let runtime = Arc::clone(&self.runtime);
        let view = self.error_view(client, Area::ThreadCommand);
        tokio::spawn(async move {
            let response = tokio::select! {
                result = runtime.run_for_user(&thread_id, &command) => match result {
                    Ok(output) => Response::Text(output),
                    Err(error) => Response::Error(view.text(&error)),
                },
                // Dropping the run kills its process group. A dropped sender (the client
                // left, or the same number started again) stops it too.
                _ = stopped => Response::Error("stopped by the user".into()),
            };
            // `stopped` is gone now, so only this run's entry is closed; a newer run under
            // the same key keeps its own.
            runs.0
                .lock()
                .retain(|entry, stop| !(entry == &key && stop.is_closed()));
            let _ = reply.send(response);
        });
    }

    /// True when the run was under way; it answers its own request with the stop.
    pub(super) fn stop_run(&self, client: ClientId, thread_id: String, run: u64) -> bool {
        self.user_runs.stop(&(client, thread_id, run))
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::oneshot::{self, error::TryRecvError};

    use super::UserRuns;

    #[test]
    fn only_the_client_that_started_a_run_can_stop_it() {
        let runs = UserRuns::default();
        let (stop, mut stopped) = oneshot::channel();
        runs.0.lock().insert((1, "t".to_owned(), 7), stop);
        assert!(!runs.stop(&(2, "t".to_owned(), 7)));
        assert!(!runs.stop(&(1, "other".to_owned(), 7)));
        assert_eq!(stopped.try_recv(), Err(TryRecvError::Empty));
        assert!(runs.stop(&(1, "t".to_owned(), 7)));
        assert_eq!(stopped.try_recv(), Ok(()));
    }
}
