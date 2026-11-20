//! The core served on its private socket: who gets in, who is refused, and what a refused
//! client learns.
#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use dusk_core::{
    app::{Paths, start_hub},
    client::{Client, Hub, Scope},
};
use dusk_protocol::client::{
    Claim, Event, Greeting, Hello, OLDEST_PROTOCOL, OwnerKind, PROTOCOL_VERSION, Proof, Refusal,
    Request, Response, Then, client_proof, core_proof, nonce, proof_matches,
};
use dusk_server::local::{CoreLock, RunFiles, Serving, serve, write_key};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{
        UnixStream,
        unix::{OwnedReadHalf, OwnedWriteHalf},
    },
};

/// A core served on a socket in a fresh, short folder.
struct Core {
    root: PathBuf,
    files: RunFiles,
    key: [u8; 32],
    hub: Arc<Hub>,
    owner: Client,
    serving: Option<Serving>,
    lock: CoreLock,
}

impl Drop for Core {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn run_files() -> (PathBuf, RunFiles) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("dl-{}-{n}", std::process::id()));
    let dir = root.join("run");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let files = RunFiles {
        lock: dir.join("core.lock"),
        socket: Ok(dir.join("core.sock")),
        key: dir.join("core.key"),
        dir,
    };
    (root, files)
}

async fn core() -> Core {
    let (root, files) = run_files();
    let lock = CoreLock::try_take(&files).unwrap().unwrap();
    let key = write_key(&lock, &files).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    let owner = hub
        .attach_owner(Hello::new("owner"), Scope::full())
        .unwrap();
    let serving = serve(&lock, Arc::clone(&hub), files.clone(), key, OwnerKind::Tui)
        .await
        .unwrap();
    Core {
        root,
        files,
        key,
        hub,
        owner,
        serving: Some(serving),
        lock,
    }
}

impl Core {
    fn socket(&self) -> PathBuf {
        self.files.socket_path().unwrap().to_path_buf()
    }

    async fn connect(&self) -> Peer {
        let stream = UnixStream::connect(self.socket()).await.unwrap();
        let (read, write) = stream.into_split();
        Peer {
            read: BufReader::new(read),
            write,
            heard: String::new(),
        }
    }
}

/// A raw client: what it says and everything it heard.
struct Peer {
    read: BufReader<OwnedReadHalf>,
    write: OwnedWriteHalf,
    heard: String,
}

impl Peer {
    async fn say(&mut self, line: &str) {
        self.write.write_all(line.as_bytes()).await.unwrap();
        self.write.write_all(b"\n").await.unwrap();
    }

    async fn say_json(&mut self, value: &impl serde::Serialize) {
        self.say(&serde_json::to_string(value).unwrap()).await;
    }

    /// The next line, or `None` once the core closed the connection.
    async fn line(&mut self) -> Option<String> {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), self.read.read_line(&mut line))
            .await
            .expect("a line or the end within 5 s");
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => {
                self.heard.push_str(&line);
                Some(line)
            }
        }
    }

    async fn greeting(&mut self) -> Greeting {
        let line = self.line().await.expect("a greeting");
        serde_json::from_str(&line).unwrap_or_else(|_| panic!("a greeting: {line}"))
    }

    /// Whether the core ends the connection without another greeting.
    async fn ends(&mut self) -> bool {
        while let Some(line) = self.line().await {
            if serde_json::from_str::<Greeting>(&line).is_ok() {
                return false;
            }
        }
        true
    }
}

fn hello(auth: Option<Claim>, nc: &str) -> Hello {
    Hello {
        protocol: PROTOCOL_VERSION,
        client: "app".to_owned(),
        auth,
        nonce: nc.to_owned(),
    }
}

fn refused(greeting: &Greeting) -> Refusal {
    match greeting {
        Greeting::Refused { code, .. } => *code,
        other => panic!("refused, not {other:?}"),
    }
}

/// The handshake of an app holding `key`: it checks the core's proof before its own.
async fn handshake(core: &Core, key: &[u8; 32], then: Then) -> (Peer, Greeting, Proof) {
    let mut peer = core.connect().await;
    let nc = nonce();
    peer.say_json(&hello(Some(Claim::Key), &nc)).await;
    let Greeting::Challenge { nonce: ns, proof } = peer.greeting().await else {
        panic!("a challenge");
    };
    assert!(proof_matches(
        &core_proof(&core.key, "key", &nc, &ns),
        &proof
    ));
    let answer = Proof {
        proof: client_proof(key, "key", &nc, &ns),
        then,
    };
    peer.say_json(&answer).await;
    let last = peer.greeting().await;
    (peer, last, answer)
}

#[tokio::test]
async fn a_client_with_the_key_attaches_and_lists_threads() {
    let core = core().await;
    let (peer, welcome, _) = handshake(&core, &core.key, Then::Attach).await;
    let Greeting::Welcome {
        core: info,
        client_id,
        scope,
    } = welcome
    else {
        panic!("welcome, not {welcome:?}");
    };
    assert_eq!(
        (info.protocol, info.oldest, info.owner, scope.as_str()),
        (PROTOCOL_VERSION, OLDEST_PROTOCOL, OwnerKind::Tui, "full")
    );
    let client = Client::over(peer.read, peer.write).await;
    assert_eq!(client.threads().list().await.unwrap(), vec![]);
    let Response::Clients(listed) = core.owner.request(Request::Clients).await else {
        panic!("a list of clients");
    };
    let app = listed.iter().find(|c| c.id == client_id).expect("listed");
    assert_eq!(
        (app.label.as_str(), app.transport.as_str()),
        ("app", "socket")
    );
}

#[tokio::test]
async fn a_wrong_proof_is_refused_and_counted() {
    let core = core().await;
    let key_hex: String = core.key.iter().map(|b| format!("{b:02x}")).collect();
    for _ in 0..5 {
        let (mut peer, last, _) = handshake(&core, &[7; 32], Then::Attach).await;
        assert_eq!(refused(&last), Refusal::Auth);
        let Greeting::Refused { reason, .. } = &last else {
            unreachable!()
        };
        assert_eq!(reason, &Refusal::Auth.to_string());
        assert!(peer.ends().await);
        // What a client without the key heard holds nothing it could use.
        assert!(!peer.heard.contains(&key_hex));
    }
    // Then even the key holder waits.
    let mut peer = core.connect().await;
    peer.say_json(&hello(Some(Claim::Key), &nonce())).await;
    assert_eq!(refused(&peer.greeting().await), Refusal::Cooldown);
    assert!(peer.ends().await);
}

#[tokio::test]
async fn a_replayed_proof_is_refused() {
    let core = core().await;
    let mut first = core.connect().await;
    let nc = nonce();
    first.say_json(&hello(Some(Claim::Key), &nc)).await;
    let Greeting::Challenge { nonce: ns, .. } = first.greeting().await else {
        panic!("a challenge");
    };
    let recorded = Proof {
        proof: client_proof(&core.key, "key", &nc, &ns),
        then: Then::Attach,
    };
    first.say_json(&recorded).await;
    assert!(matches!(first.greeting().await, Greeting::Welcome { .. }));
    // The same hello and proof on another connection: the core's nonce is new.
    let mut replay = core.connect().await;
    replay.say_json(&hello(Some(Claim::Key), &nc)).await;
    let Greeting::Challenge { nonce: fresh, .. } = replay.greeting().await else {
        panic!("a challenge");
    };
    assert_ne!(fresh, ns);
    replay.say_json(&recorded).await;
    assert_eq!(refused(&replay.greeting().await), Refusal::Auth);
}

#[tokio::test]
async fn a_token_claim_is_refused_in_this_phase() {
    let core = core().await;
    let mut peer = core.connect().await;
    let token = Claim::Token {
        id: "dkt_abc".to_owned(),
    };
    peer.say_json(&hello(Some(token), &nonce())).await;
    assert_eq!(refused(&peer.greeting().await), Refusal::Auth);
    assert!(peer.ends().await);
    // No claim at all is refused the same way.
    let mut peer = core.connect().await;
    peer.say_json(&hello(None, &nonce())).await;
    assert_eq!(refused(&peer.greeting().await), Refusal::Auth);
}

#[tokio::test]
async fn a_bad_client_nonce_gets_no_proof() {
    let core = core().await;
    for nc in ["", "abc", &"A".repeat(64), &format!("{}key", nonce())] {
        let mut peer = core.connect().await;
        peer.say_json(&hello(Some(Claim::Key), nc)).await;
        assert_eq!(refused(&peer.greeting().await), Refusal::Auth, "{nc}");
        assert!(peer.ends().await);
    }
}

#[tokio::test]
async fn a_hello_of_another_version_is_refused_with_versions() {
    let core = core().await;
    // A future hello this core cannot parse: a claim it does not know, fields it lacks.
    let future =
        r#"{"protocol":999,"client":"app","auth":{"Passkey":{"id":"x"}},"nonce":"zz","more":1}"#;
    // An old hello without the key learns the versions before any proof.
    let token = Claim::Token {
        id: "dkt_abc".to_owned(),
    };
    let old = serde_json::to_string(&Hello {
        protocol: OLDEST_PROTOCOL - 1,
        ..hello(Some(token), &nonce())
    })
    .unwrap();
    for (line, theirs) in [(future.to_owned(), 999), (old, OLDEST_PROTOCOL - 1)] {
        let mut peer = core.connect().await;
        peer.say(&line).await;
        let Greeting::Refused { code, reason, core } = peer.greeting().await else {
            panic!("refused");
        };
        assert_eq!(code, Refusal::Version);
        assert_eq!(core.protocol, PROTOCOL_VERSION);
        assert_eq!(
            reason,
            format!(
                "the core speaks {OLDEST_PROTOCOL} to {PROTOCOL_VERSION}, this client {theirs}; \
                 update duskcode"
            )
        );
        assert!(peer.ends().await);
    }
}

/// The handshake of a key holder that speaks `protocol`.
async fn handshake_as(core: &Core, protocol: u32, then: Then) -> Greeting {
    let mut peer = core.connect().await;
    let nc = nonce();
    peer.say_json(&Hello {
        protocol,
        ..hello(Some(Claim::Key), &nc)
    })
    .await;
    let Greeting::Challenge { nonce: ns, .. } = peer.greeting().await else {
        panic!("a challenge");
    };
    let proof = Proof {
        proof: client_proof(&core.key, "key", &nc, &ns),
        then,
    };
    peer.say_json(&proof).await;
    peer.greeting().await
}

#[tokio::test]
async fn a_key_holder_of_another_version_may_stop_but_not_attach() {
    let mut core = core().await;
    // It learns the versions from a core that proved itself, and is not attached.
    for protocol in [OLDEST_PROTOCOL, PROTOCOL_VERSION + 1] {
        let last = handshake_as(&core, protocol, Then::Attach).await;
        let Greeting::Refused {
            code, core: info, ..
        } = last
        else {
            panic!("refused");
        };
        assert_eq!((code, info.protocol), (Refusal::Version, PROTOCOL_VERSION));
    }
    // A newer app may stop an older core (spec 3.6); which app may is the app's rule.
    let last = handshake_as(&core, PROTOCOL_VERSION + 1, Then::Stop).await;
    assert!(matches!(last, Greeting::Welcome { .. }), "{last:?}");
    let event = tokio::time::timeout(Duration::from_secs(5), core.owner.events.recv())
        .await
        .expect("an event within 5 s");
    assert!(matches!(event, Some(Event::CoreStopping { .. })));
}

#[tokio::test]
async fn an_unknown_then_is_refused() {
    let core = core().await;
    let mut peer = core.connect().await;
    let nc = nonce();
    peer.say_json(&hello(Some(Claim::Key), &nc)).await;
    let Greeting::Challenge { nonce: ns, .. } = peer.greeting().await else {
        panic!("a challenge");
    };
    let proof = client_proof(&core.key, "key", &nc, &ns);
    peer.say(&format!(r#"{{"proof":"{proof}","then":"Teleport"}}"#))
        .await;
    assert_eq!(refused(&peer.greeting().await), Refusal::Version);
    assert!(peer.ends().await);
}

#[tokio::test]
async fn a_garbage_or_oversized_first_line_gets_nothing() {
    let core = core().await;
    let mut peer = core.connect().await;
    peer.say("hello there").await;
    assert!(peer.ends().await);
    let mut peer = core.connect().await;
    // The core may close before it has all of it; what it says matters, not the write.
    let _ = peer.write.write_all(&[b'x'; 64 * 1024]).await;
    assert!(peer.ends().await);
    // A first line that is a request, not a hello, is not served either.
    let mut peer = core.connect().await;
    peer.say(r#"{"id":1,"request":"Shutdown"}"#).await;
    assert!(peer.ends().await);
}

#[tokio::test]
async fn stop_with_the_key_stops_the_core() {
    let mut core = core().await;
    let (mut peer, last, _) = handshake(&core, &core.key, Then::Stop).await;
    assert!(matches!(last, Greeting::Welcome { .. }), "{last:?}");
    assert!(peer.ends().await);
    let event = tokio::time::timeout(Duration::from_secs(5), core.owner.events.recv())
        .await
        .expect("an event within 5 s");
    assert_eq!(
        event,
        Some(Event::CoreStopping {
            reason: "stopped by duskcode stop".to_owned()
        })
    );
}

#[tokio::test]
async fn the_sixty_fifth_connection_is_dropped() {
    let core = core().await;
    let mut waiting = Vec::new();
    for _ in 0..64 {
        let mut peer = core.connect().await;
        peer.say_json(&hello(Some(Claim::Key), &nonce())).await;
        assert!(matches!(peer.greeting().await, Greeting::Challenge { .. }));
        waiting.push(peer);
    }
    let mut extra = core.connect().await;
    assert_eq!(extra.line().await, None);
    drop(waiting);
}

#[tokio::test]
async fn the_socket_goes_on_stop_and_a_stale_one_is_replaced_by_the_lock_holder() {
    let mut core = core().await;
    let socket = core.socket();
    core.serving.take().unwrap().stop().await;
    assert!(!socket.exists());
    assert!(UnixStream::connect(&socket).await.is_err());

    // A crashed core left its socket behind.
    drop(std::os::unix::net::UnixListener::bind(&socket).unwrap());
    assert!(
        fs::symlink_metadata(&socket)
            .unwrap()
            .file_type()
            .is_socket()
    );
    // A lock of another core cannot take it.
    let (other_root, other) = run_files();
    let other_lock = CoreLock::try_take(&other).unwrap().unwrap();
    let wrong = serve(
        &other_lock,
        Arc::clone(&core.hub),
        core.files.clone(),
        core.key,
        OwnerKind::Tui,
    )
    .await;
    assert!(wrong.is_err());
    assert!(socket.exists());
    let _ = fs::remove_dir_all(&other_root);
    // The holder of this core's lock replaces it, private.
    let serving = serve(
        &core.lock,
        Arc::clone(&core.hub),
        core.files.clone(),
        core.key,
        OwnerKind::Tui,
    )
    .await
    .unwrap();
    use std::os::unix::fs::MetadataExt;
    assert_eq!(fs::metadata(&socket).unwrap().mode() & 0o777, 0o600);
    let (_, last, _) = handshake(&core, &core.key, Then::Attach).await;
    assert!(matches!(last, Greeting::Welcome { .. }));
    // Dropping the handle stops serving too.
    drop(serving);
    for _ in 0..100 {
        if !socket.exists() {
            break;
        }
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(!socket.exists());
}
