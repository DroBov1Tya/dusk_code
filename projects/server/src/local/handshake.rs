//! The core's side of the handshake on the socket: check the version and the name before
//! any secret is used, prove the core holds the key, then check the client's proof.
//! The key itself never crosses the socket, and nothing is computed over a nonce of the
//! wrong shape.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use dusk_core::{
    app::VERSION,
    client::{Client, Hub, Scope},
};
use dusk_protocol::client::{
    Claim, CoreInfo, Event, Greeting, Hello, OLDEST_PROTOCOL, OwnerKind, PROTOCOL_VERSION, Proof,
    Refusal, Then, client_proof, core_proof, nonce_is_valid, proof_matches, try_nonce,
};
use parking_lot::Mutex;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use super::lines::Lines;
use crate::stream::Frames;

/// How long a client has for each of its two handshake lines.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(5);

/// How long the owner's user has to answer a handover; the asking app waits as long.
const HANDOVER: Duration = Duration::from_secs(120);

/// The longest handshake line a client may send; both are far shorter.
pub(super) const MAX_HELLO: usize = 4096;

/// What a failed proof costs the connection that sent it, before its refusal. The key is
/// 32 random bytes, so this is not against guessing: it only makes a loop of bad proofs
/// slow, and it never touches another connection.
pub(super) const FAILED_PROOF_DELAY: Duration = Duration::from_secs(1);

/// How often failed proofs are reported in the log at most.
const FAILURE_REPORT: Duration = Duration::from_secs(60);

/// The proof id of the core's key.
const KEY_ID: &str = "key";

/// What the core answers with: whom it serves, and what it checks clients against.
pub(super) struct Gate {
    pub hub: Arc<Hub>,
    pub key: Key,
    pub owner: OwnerKind,
    /// Failed proofs, counted for the log only: they never refuse another client, or any
    /// process of this user could lock the user's own apps out.
    pub failures: Failures,
}

/// The core's key; never shown, whatever derives `Debug` around it.
pub(super) struct Key(pub [u8; 32]);

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key(..)")
    }
}

/// Failed proofs since the last report, and when that report was.
#[derive(Default)]
pub(super) struct Failures(Mutex<(u64, Option<Instant>)>);

impl Failures {
    /// Counts one, and reports the count at most once a minute.
    fn failed(&self) {
        let mut state = self.0.lock();
        state.0 += 1;
        let due = state.1.is_none_or(|last| last.elapsed() >= FAILURE_REPORT);
        if due {
            warn!(
                count = state.0,
                "socket clients failed to prove they hold the core key"
            );
            *state = (0, Some(Instant::now()));
        }
    }
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
        if !nonce_is_valid(&hello.nonce) {
            return self.refuse_plain(lines, Refusal::Auth).await;
        }
        let ns = try_nonce().map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
        let challenge = Greeting::Challenge {
            proof: core_proof(&self.key.0, KEY_ID, &hello.nonce, &ns),
            nonce: ns.clone(),
        };
        say(lines, &challenge).await?;
        let expected = client_proof(&self.key.0, KEY_ID, &hello.nonce, &ns);
        let proof = serde_json::from_str::<Proof>(&read(lines).await?)
            .ok()
            .filter(|proof| proof_matches(&expected, &proof.proof));
        let Some(proof) = proof else {
            self.failures.failed();
            debug!(name = %hello.client, "a socket client failed to prove it holds the key");
            tokio::time::sleep(FAILED_PROOF_DELAY).await;
            return self.refuse_plain(lines, Refusal::Auth).await;
        };
        // An older app never stops or hands over a newer core; the app's own rule, held
        // here too.
        let older = hello.protocol < PROTOCOL_VERSION;
        if older && matches!(proof.then, Then::Stop | Then::Handover) {
            return self
                .refuse(lines, Refusal::Version, versions(hello.protocol))
                .await;
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
                "the core speaks {PROTOCOL_VERSION}, this client {}; {}",
                hello.protocol,
                update(hello.protocol)
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
        // The frozen hello carries no app version, so the owner is told the protocol, as
        // `protocol N`; the owner's dialog words it.
        let asking = format!("protocol {}", hello.protocol);
        info!(name = %hello.client, %asking, "a newer app asks for the core");
        let answer = self.hub.ask_handover(asking);
        let accepted = tokio::select! {
            answer = answer => answer.unwrap_or(false),
            // The asking app says nothing more; a line or its leaving ends the wait.
            _ = lines.next() => false,
            () = tokio::time::sleep(HANDOVER) => false,
        };
        if !accepted {
            // An ask nobody waits for any more must not stay up in the owner's window.
            self.hub.withdraw_handover();
            return self.refuse_plain(lines, Refusal::Declined).await;
        }
        self.welcome(lines, 0).await?;
        Ok(None)
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

    /// `Refused { Full }` as one line, for a client turned away before its handshake.
    pub(super) fn full_line(&self) -> String {
        let refused = Greeting::Refused {
            code: Refusal::Full,
            reason: Refusal::Full.to_string(),
            core: self.info(),
        };
        let mut line = serde_json::to_string(&refused).unwrap_or_default();
        line.push('\n');
        line
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
/// Why the versions do not meet, and which side to update: an older client cannot be
/// served by this core, a newer one needs a newer core.
fn versions(theirs: u32) -> String {
    let update = update(theirs);
    format!(
        "the core speaks {OLDEST_PROTOCOL} to {PROTOCOL_VERSION}, this client {theirs}; {update}"
    )
}

/// Which side to update: an older client than this core, or the core itself.
fn update(theirs: u32) -> &'static str {
    match theirs < PROTOCOL_VERSION {
        true => "update the client",
        false => "update duskcode",
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_core::app::{Paths, start_hub};
    use dusk_protocol::client::nonce;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream, duplex};

    async fn gate(name: &str) -> Gate {
        let root = std::env::temp_dir().join(format!("dusk-hs-{}-{name}", std::process::id()));
        let paths = Paths::new(root.join("config"), root.join("work"));
        let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
        Gate {
            hub,
            key: Key([3; 32]),
            owner: OwnerKind::Tui,
            failures: Failures::default(),
        }
    }

    fn lines() -> (Lines<DuplexStream>, DuplexStream) {
        let (near, far) = duplex(1 << 16);
        (Lines::new(near, MAX_HELLO), far)
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_client_that_says_too_little_is_let_go_after_five_seconds() {
        let gate = gate("silent").await;
        tokio::time::pause();
        // Nothing at all, half a hello, and a hello with no proof after it.
        let key_hello = serde_json::to_string(&Hello {
            auth: Some(Claim::Key),
            nonce: nonce(),
            ..Hello::new("app")
        })
        .unwrap();
        for said in [
            String::new(),
            r#"{"protocol":37"#.to_owned(),
            key_hello + "\n",
        ] {
            let (mut lines, mut far) = lines();
            far.write_all(said.as_bytes()).await.unwrap();
            let start = tokio::time::Instant::now();
            assert!(gate.admit(&mut lines).await.is_err(), "{said}");
            assert!(start.elapsed() >= HANDSHAKE, "{said}");
            assert!(start.elapsed() < HANDSHAKE * 2, "{said}");
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_failed_proof_costs_its_connection_a_delay() {
        let gate = gate("delay").await;
        tokio::time::pause();
        let (mut lines, far) = lines();
        let (read, mut write) = tokio::io::split(far);
        let mut read = BufReader::new(read);
        let hello = Hello {
            auth: Some(Claim::Key),
            nonce: nonce(),
            ..Hello::new("app")
        };
        let client = async move {
            let mut line = serde_json::to_string(&hello).unwrap();
            line.push('\n');
            write.write_all(line.as_bytes()).await.unwrap();
            let mut challenge = String::new();
            read.read_line(&mut challenge).await.unwrap();
            let start = tokio::time::Instant::now();
            write
                .write_all(b"{\"proof\":\"00\",\"then\":\"Attach\"}\n")
                .await
                .unwrap();
            let mut answer = String::new();
            read.read_line(&mut answer).await.unwrap();
            (start.elapsed(), answer)
        };
        let (admitted, (waited, answer)) = tokio::join!(gate.admit(&mut lines), client);
        assert!(matches!(admitted, Ok(None)));
        assert!(waited >= FAILED_PROOF_DELAY, "{waited:?}");
        assert!(answer.contains(r#""code":"Auth""#), "{answer}");
    }

    #[test]
    fn the_key_is_never_shown() {
        assert_eq!(format!("{:?}", Key([9; 32])), "Key(..)");
    }
}
