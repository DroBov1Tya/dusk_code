//! The handshake: the first lines a client and the core say to each other, and the versions
//! they speak. These shapes are frozen once released, because they are what two versions of
//! duskcode say before either knows the other; a test pins them.
//!
//! No secret crosses the wire: both sides prove they hold the same key by HMAC over two fresh
//! nonces, the core first, so a process squatting on the socket learns nothing it can replay.

mod proof;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};

pub use proof::{client_proof, core_proof, nonce_is_valid, proof_matches, token_key};
#[cfg(not(target_arch = "wasm32"))]
pub use proof::{nonce, try_nonce};

/// Raised whenever a request, a response or an event changes shape.
pub const PROTOCOL_VERSION: u32 = 39;

/// The oldest protocol the core still accepts from an external client; raised only on a
/// change that is not additive.
pub const OLDEST_PROTOCOL: u32 = 39;

/// The longest client name, in characters.
const MAX_CLIENT_NAME: usize = 40;

/// The first line a client sends.
///
/// A listener reads `protocol` leniently first, through a struct with only that field, so a
/// newer client that sends a claim it does not know still gets `Refused { Version }`. A
/// client never acts on a `Refused` that came before the core's proof beyond showing it:
/// anyone on the socket can send one.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub protocol: u32,
    /// Who is asking, such as `tui`; threads it creates carry it as their source. 1 to 40
    /// of `[A-Za-z0-9 ._-]`.
    pub client: String,
    /// Which secret the client holds; absent in-process, required on every transport.
    #[serde(default)]
    pub auth: Option<Claim>,
    /// 32 random bytes, lowercase hex; empty in-process.
    #[serde(default)]
    pub nonce: String,
}

impl Hello {
    /// The hello of an in-process client: the current protocol, no claim, no nonce.
    pub fn new(client: &str) -> Hello {
        Hello {
            protocol: PROTOCOL_VERSION,
            client: client.to_owned(),
            auth: None,
            nonce: String::new(),
        }
    }

    /// Whether the core can talk to this client at all: checked on every path before any
    /// secret is used, so an older client always learns the core's version.
    pub fn accepted(&self) -> Result<(), Refusal> {
        if !(OLDEST_PROTOCOL..=PROTOCOL_VERSION).contains(&self.protocol) {
            return Err(Refusal::Version);
        }
        let length = self.client.chars().count();
        let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, ' ' | '.' | '_' | '-');
        if length == 0 || length > MAX_CLIENT_NAME || !self.client.chars().all(allowed) {
            return Err(Refusal::BadName);
        }
        Ok(())
    }
}

/// Which secret a client says it holds.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Claim {
    /// The core's own key, for the user's own apps.
    Key,
    /// A token the user made, named by its id.
    Token { id: String },
}

/// The client's second line: its own proof, and what it wants.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proof {
    pub proof: String,
    pub then: Then,
}

/// What a client wants once it has proved itself.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", schemars(extend("x-open" = true)))]
pub enum Then {
    Attach,
    /// Stop the core for everyone; only with the key.
    Stop,
    /// Ask the owner's user to restart the core for a newer app; only with the key.
    Handover,
    /// A wish this core does not know, from a newer client; the core refuses it.
    #[cfg_attr(feature = "schema", schemars(skip))]
    #[serde(other)]
    Unknown,
}

/// Every line the core sends during the handshake.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Greeting {
    /// The answer to a `Hello` that can go on: the core proves it holds the secret.
    Challenge { nonce: String, proof: String },
    Welcome {
        core: CoreInfo,
        client_id: u64,
        scope: String,
    },
    Refused {
        code: Refusal,
        reason: String,
        core: CoreInfo,
    },
}

/// What a client learns about the core it reached.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreInfo {
    /// The core's `PROTOCOL_VERSION`.
    pub protocol: u32,
    /// The core's `OLDEST_PROTOCOL`.
    pub oldest: u32,
    /// The app version, such as `0.77.0`.
    pub version: String,
    pub owner: OwnerKind,
    pub busy: Busy,
}

/// Which app started the core.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", schemars(extend("x-open" = true)))]
pub enum OwnerKind {
    Gui,
    Tui,
    Serve,
    /// An owner a newer core knows and this client does not.
    #[cfg_attr(feature = "schema", schemars(skip))]
    #[serde(other)]
    Unknown,
}

/// How much the core is doing, so a client can tell whether a restart would hurt.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Busy {
    pub clients: u32,
    pub running: u32,
}

/// Why the core will not go on with a client.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", schemars(extend("x-open" = true)))]
pub enum Refusal {
    Version,
    Auth,
    Cooldown,
    BadName,
    Full,
    Stopping,
    Declined,
    /// A reason a newer core knows and this client does not.
    #[cfg_attr(feature = "schema", schemars(skip))]
    #[serde(other)]
    Unknown,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Refusal::Version => "the client and the core speak different protocols",
            Refusal::Auth => "the client could not prove it holds the secret",
            Refusal::Cooldown => "too many failed attempts; try again later",
            Refusal::BadName => "the client name must be 1 to 40 of A-Z a-z 0-9 space . _ -",
            Refusal::Full => "the core has too many clients",
            Refusal::Stopping => "the core is stopping",
            Refusal::Declined => "the owner declined",
            Refusal::Unknown => "the core refused",
        })
    }
}
