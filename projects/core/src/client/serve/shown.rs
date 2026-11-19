//! What a client is told of a request that failed. A handler's error text can hold paths
//! and other things of this machine; a client that is not the user's own sees only the
//! sentences meant for it, and a short reference for the rest, which the log explains.

use crate::client::Area;
use tracing::warn;

/// A sentence for the user: the one kind of handler error whose text any client may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown(pub String);

impl Shown {
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }
}

impl std::fmt::Display for Shown {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Shown {}

/// The text of `error` for the client named `client` that asked in `area`. A client with
/// `raw` errors (the user's own) gets all of it; any other gets the `Shown` text the chain
/// holds, or a reference to the log line that has the whole error.
pub fn shown_text(error: &anyhow::Error, raw: bool, client: &str, area: Area) -> String {
    if raw {
        return format!("{error:#}");
    }
    if let Some(shown) = error.downcast_ref::<Shown>().or_else(|| {
        error
            .chain()
            .find_map(|cause| cause.downcast_ref::<Shown>())
    }) {
        return shown.0.clone();
    }
    let ref8 = reference();
    warn!(ref = %ref8, client, area = ?area, error = %format!("{error:#}"), "a request failed");
    format!("the request failed (ref {ref8})")
}

/// Eight random hex characters.
fn reference() -> String {
    let mut bytes = [0u8; 4];
    if getrandom::fill(&mut bytes).is_err() {
        bytes = std::process::id().to_le_bytes();
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
