//! A client attached to the core, as `Request::Clients` lists it.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachedClient {
    /// Never reused within a run; what `Request::Disconnect` names.
    pub id: u64,
    /// What the client called itself in its hello, such as `tui`.
    pub label: String,
    /// How it is connected, such as `in-process` or `socket`.
    pub transport: String,
    /// What it may do: `full`, `web` or `threads`.
    pub scope: String,
    /// When it attached, in seconds since the Unix epoch.
    pub since: i64,
}
