//! The core's side of the handshake on the socket: check the version and the name before
//! any secret is used, prove the core holds the key, then check the client's proof.
//! The key itself never crosses the socket, and nothing is computed over a nonce of the
//! wrong shape.

use std::{sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use dusk_core::{
    app::VERSION,
    client::{Client, Hub, Scope},
};
use dusk_protocol::client::{
    Claim, CoreInfo, Event, Greeting, Hello, OLDEST_PROTOCOL, OwnerKind, PROTOCOL_VERSION, Proof,
    Refusal, Then, client_proof, core_proof, nonce_is_valid, proof_matches, try_nonce,
};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info};

use super::lines::Lines;
use crate::{auth::Limiter, stream::Frames};

/// How long a client has for each of its two handshake lines.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(5);

/// How long the owner's user has to answer a handover; the asking app waits as long.
const HANDOVER: Duration = Duration::from_secs(120);

/// The longest handshake line a client may send; both are far shorter.
pub(super) const MAX_HELLO: usize = 4096;

/// The proof id of the core's key.
const KEY_ID: &str = "key";

/// What the core answers with: whom it serves, and what it checks clients against.
pub(super) struct Gate {
    pub hub: Arc<Hub>,
    pub key: [u8; 32],
    pub owner: OwnerKind,
    /// Failed proofs; five in a minute and every handshake waits.
    pub failures: Limiter,
}

/// A client let in to be carried by the pump.
pub(super) struct Admitted {
    pub client: Arc<Client>,
    pub events: mpsc::UnboundedReceiver<Event>,
}

/// The first line read only for its protocol, so a hello of another version that this one
/// cannot parse still gets a refusal naming the versions.
#[derive(Deserialize)]
struct Version {
    protocol: u32,
}

/// The limiter keeps one count for the socket: every peer on it is this user.
const PEER: &str = "socket";

impl Gate {
    /// Runs the handshake; `Some` when the client is attached and the stream goes on.
    pub(super) async fn admit<S>(&self, lines: &mut Lines<S>) -> Result<Option<Admitted>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        let first = read(lines).await?;
        let hello = match serde_json::from_str::<Hello>(&first) {
            Ok(hello) => hello,
            // A hello of another version that this one cannot parse still learns the versions.
            Err(_) => match serde_json::from_str::<Version>(&first) {
                Ok(Version { protocol }) if !speaks(protocol) => {
                    return self
                        .refuse(lines, Refusal::Version, versions(protocol))
                        .await;
                }
                _ => bail!("the first line is not a hello"),
            },
        };
        // The name is checked here; the version below, where it depends on the claim.
        let name = Hello {
            protocol: PROTOCOL_VERSION,
            ..hello.clone()
        };
        if let Err(code) = name.accepted() {
            return self.refuse_plain(lines, code).await;
        }
        // Only the key in this phase. A key holder of another version goes on, so it can
        // stop or hand over the core and learns the versions from a core that proved itself.
        if hello.auth != Some(Claim::Key) {
            return match speaks(hello.protocol) {
                true => self.refuse_plain(lines, Refusal::Auth).await,
                false => {
                    let reason = versions(hello.protocol);
                    self.refuse(lines, Refusal::Version, reason).await
                }
            };
        }
        if self.failures.blocked(PEER).is_some() {
            return self.refuse_plain(lines, Refusal::Cooldown).await;
        }
        if !nonce_is_valid(&hello.nonce) {
            return self.refuse_plain(lines, Refusal::Auth).await;
        }
        let ns = try_nonce().map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
        let challenge = Greeting::Challenge {
            proof: core_proof(&self.key, KEY_ID, &hello.nonce, &ns),
            nonce: ns.clone(),
        };
        say(lines, &challenge).await?;
        let Ok(proof) = serde_json::from_str::<Proof>(&read(lines).await?) else {
            self.failures.failed(PEER);
            return self.refuse_plain(lines, Refusal::Auth).await;
        };
        let expected = client_proof(&self.key, KEY_ID, &hello.nonce, &ns);
        if !proof_matches(&expected, &proof.proof) {
            self.failures.failed(PEER);
            info!(name = %hello.client, "a socket client failed to prove it holds the key");
            return self.refuse_plain(lines, Refusal::Auth).await;
        }
        match proof.then {
            Then::Attach => self.attach(lines, hello).await,
            Then::Stop => {
                info!(name = %hello.client, "the core is stopped from the socket");
                // Proven: the core stops even when the client left before the answer.
                let answered = self.welcome(lines, 0).await;
                self.hub.stop("stopped by duskcode stop");
                answered.map(|()| None)
            }
            Then::Handover => self.handover(lines, &hello).await,
            Then::Unknown => {
                let reason = "this core does not know what the client asked for".to_owned();
                self.refuse(lines, Refusal::Version, reason).await
            }
        }
    }

    async fn attach<S>(&self, lines: &mut Lines<S>, hello: Hello) -> Result<Option<Admitted>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        // An app attached here speaks the frames of this core exactly; one of another
        // version learns the core's and decides what to do from there.
        if hello.protocol != PROTOCOL_VERSION {
            let reason = format!(
                "the core speaks {PROTOCOL_VERSION}, this app {}",
                hello.protocol
            );
            return self.refuse(lines, Refusal::Version, reason).await;
        }
        let name = hello.client.clone();
        let Ok(mut client) = self.hub.attach_labelled(hello, Scope::full(), "socket") else {
            return self.refuse_plain(lines, Refusal::Stopping).await;
        };
        let id = client.id().unwrap_or_default();
        info!(client = id, transport = "socket", %name, "attached");
        self.welcome(lines, id).await?;
        let events = std::mem::replace(&mut client.events, mpsc::unbounded_channel().1);
        Ok(Some(Admitted {
            client: Arc::new(client),
            events,
        }))
    }

    async fn handover<S>(&self, lines: &mut Lines<S>, hello: &Hello) -> Result<Option<Admitted>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        // The hello names no app version; its protocol is what the owner is told.
        let asking = format!("protocol {}", hello.protocol);
        info!(name = %hello.client, %asking, "a newer app asks for the core");
        let answer = self.hub.ask_handover(asking);
        match tokio::time::timeout(HANDOVER, answer).await {
            Ok(Ok(true)) => {
                self.welcome(lines, 0).await?;
                Ok(None)
            }
            _ => self.refuse_plain(lines, Refusal::Declined).await,
        }
    }

    fn info(&self) -> CoreInfo {
        CoreInfo {
            protocol: PROTOCOL_VERSION,
            oldest: OLDEST_PROTOCOL,
            version: VERSION.to_owned(),
            owner: self.owner,
            busy: *self.hub.busy().borrow(),
        }
    }

    async fn welcome<S>(&self, lines: &mut Lines<S>, client_id: u64) -> Result<()>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        let welcome = Greeting::Welcome {
            core: self.info(),
            client_id,
            scope: "full".to_owned(),
        };
        say(lines, &welcome).await
    }

    /// Refuses with the fixed sentence of `code`.
    async fn refuse_plain<S>(&self, lines: &mut Lines<S>, code: Refusal) -> Result<Option<Admitted>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        self.refuse(lines, code, code.to_string()).await
    }

    async fn refuse<S>(
        &self,
        lines: &mut Lines<S>,
        code: Refusal,
        reason: String,
    ) -> Result<Option<Admitted>>
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
    {
        debug!(?code, "a socket client is refused");
        let refused = Greeting::Refused {
            code,
            reason,
            core: self.info(),
        };
        say(lines, &refused).await?;
        Ok(None)
    }
}

/// Whether the core talks to a client of `protocol` at all.
fn speaks(protocol: u32) -> bool {
    (OLDEST_PROTOCOL..=PROTOCOL_VERSION).contains(&protocol)
}

/// The refusal of a client of `theirs`, naming both sides.
fn versions(theirs: u32) -> String {
    format!(
        "the core speaks {OLDEST_PROTOCOL} to {PROTOCOL_VERSION}, this client {theirs}; update \
         duskcode"
    )
}

async fn read<S>(lines: &mut Lines<S>) -> Result<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    tokio::time::timeout(HANDSHAKE, lines.next())
        .await
        .context("the client said nothing in time")??
        .context("the client left during the handshake")
}

async fn say<S>(lines: &mut Lines<S>, greeting: &Greeting) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let text = serde_json::to_string(greeting).context("cannot write a greeting")?;
    tokio::time::timeout(HANDSHAKE, lines.send(text))
        .await
        .context("the client does not read")?
}
