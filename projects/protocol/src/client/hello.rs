//! The first thing a client says, and the version it must speak.

use serde::{Deserialize, Serialize};

/// Raised whenever a request, a response or an event changes shape.
pub const PROTOCOL_VERSION: u32 = 7;

/// The first thing a client says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// Who is asking, such as `tui`; threads it creates carry it as their source.
    pub client: String,
}
