//! The user's own shell in a terminal the core runs.

use super::Client;
use anyhow::Result;
use dusk_protocol::client::{Request, TerminalRequest};
use std::path::PathBuf;

/// The terminals this client opened. Their output comes as `Event::Terminal`, their end
/// as `Event::TerminalClosed`, among the client's events.
pub struct Terminal<'a>(pub(super) &'a Client);

impl Terminal<'_> {
    /// Starts the shell in `cwd`, a folder inside a saved project or the workspace;
    /// answers with the terminal's id.
    pub async fn open(&self, cwd: PathBuf, cols: u16, rows: u16) -> Result<String> {
        let request = TerminalRequest::Open { cwd, cols, rows };
        self.0.text(Request::Terminal(request)).await
    }

    /// Keys and pasted text, at most `MAX_TERMINAL_INPUT` bytes a call.
    pub async fn input(&self, id: &str, data: Vec<u8>) -> Result<()> {
        let id = id.to_owned();
        let request = TerminalRequest::Input { id, data };
        self.0.done(Request::Terminal(request)).await
    }

    pub async fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let id = id.to_owned();
        let request = TerminalRequest::Resize { id, cols, rows };
        self.0.done(Request::Terminal(request)).await
    }

    pub async fn close(&self, id: &str) -> Result<()> {
        let id = id.to_owned();
        self.0
            .done(Request::Terminal(TerminalRequest::Close { id }))
            .await
    }
}
