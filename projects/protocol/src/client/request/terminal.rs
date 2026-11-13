//! Terminals: the user's own shell, run by the core and drawn by the client.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The terminals one client may have open at once.
pub const MAX_TERMINALS: usize = 16;

/// The most columns or rows a terminal has; a size is brought into `1..=MAX_TERMINAL_SIDE`.
pub const MAX_TERMINAL_SIDE: u16 = 500;

/// The most bytes one `Input` carries.
pub const MAX_TERMINAL_INPUT: usize = 64 * 1024;

/// A terminal belongs to the client that opened it: its output comes as events to that
/// client only, and no other client can name it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalRequest {
    /// Starts the user's shell in `cwd`, a folder inside a saved project or the
    /// workspace; answers with the terminal's id as text.
    Open {
        cwd: PathBuf,
        cols: u16,
        rows: u16,
    },
    /// Keys and pasted text, as the bytes the shell reads.
    Input {
        id: String,
        data: Vec<u8>,
    },
    Resize {
        id: String,
        cols: u16,
        rows: u16,
    },
    /// Ends the shell; `TerminalClosed` follows once it is gone.
    Close {
        id: String,
    },
}
