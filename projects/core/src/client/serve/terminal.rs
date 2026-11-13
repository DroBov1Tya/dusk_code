//! Requests about the terminals of the client that asks.

use super::Server;
use crate::terminal;
use anyhow::{Context, Result};
use dusk_protocol::client::{Response, TerminalRequest};

impl Server {
    pub(super) async fn terminal(&self, request: TerminalRequest) -> Result<Response> {
        let TerminalRequest::Open { cwd, cols, rows } = request else {
            self.terminals.handle(self.caller, request)?;
            return Ok(Response::Done);
        };
        let mut roots = vec![self.runtime.paths().workspace_root().to_path_buf()];
        let projects = self.runtime.state().list_projects().await?;
        roots.extend(projects.into_iter().map(|project| project.dir));
        let cwd = terminal::working_dir(&cwd, &roots).await?;
        // The caller's own queue, not every subscriber's: a shell is nobody else's to read.
        let events = self
            .subscribers
            .lock()
            .iter()
            .find(|(client, _)| *client == self.caller)
            .map(|(_, queue)| queue.clone())
            .context("the client is gone")?;
        let id = self.terminals.open(self.caller, &cwd, cols, rows, events)?;
        Ok(Response::Text(id))
    }
}
