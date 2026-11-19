//! A plugin's way out: its programs reach no network but this, a proxy the core runs on
//! this machine for that plugin alone. It lets requests out only to the origins the
//! plugin was allowed, only to public addresses, and puts the plugin's keys into them on
//! the way, each only for the origins it was given for. So a key the plugin never sees
//! can never travel anywhere else.
//!
//! A program proves itself with `Proxy-Authorization: Basic` of `dusk:<token>`, which
//! the proxy URL in its environment carries. Two ways through:
//!
//! - `METHOD https://origin/path HTTP/1.1`, the absolute form: the broker sends it on
//!   with `{{key:<name>}}` in the URL and header values replaced, and answers with the
//!   whole response; redirects come back as they are;
//! - `CONNECT host:443`: a tunnel for a library that speaks TLS itself, to an allowed
//!   origin only; nothing is put into what goes through it.
//!
//! - `policy`: what may go out, and the keys;
//! - `forward`: one request sent on;
//! - `tunnel`: one `CONNECT`.

mod forward;
mod policy;
mod tunnel;

use std::{net::SocketAddr, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use dusk_protocol::client::Secret;
use parking_lot::RwLock;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tracing::debug;

pub use policy::{Key, Origin, Policy};

/// A request's line and headers; more is not a request a plugin needs to send.
const MAX_HEAD: usize = 64 * 1024;
const MAX_HEADERS: usize = 64;
const HEAD_TIME: Duration = Duration::from_secs(10);

/// The proxy of one plugin; it stops when dropped.
pub struct Broker {
    port: u16,
    token: Secret,
    policy: Arc<RwLock<Arc<Policy>>>,
    task: JoinHandle<()>,
}

impl Drop for Broker {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Broker {
    /// Listens on a free port of this machine. Call inside the tokio runtime.
    pub fn start(policy: Policy) -> Result<Self> {
        let listener =
            std::net::TcpListener::bind("127.0.0.1:0").context("cannot open the plugin's proxy")?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let listener = TcpListener::from_std(listener)?;
        let token = policy.token.clone();
        let policy = Arc::new(RwLock::new(Arc::new(policy)));
        let task = tokio::spawn(accept(listener, Arc::clone(&policy)));
        Ok(Self {
            port,
            token,
            policy,
            task,
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// The token stays: programs already running keep theirs.
    pub fn set_policy(&self, mut policy: Policy) {
        policy.token = self.token.clone();
        *self.policy.write() = Arc::new(policy);
    }

    /// What a program of the plugin is given: the proxy, plain and as the standard
    /// proxy variables with the token as the password.
    pub fn env(&self) -> Vec<(String, String)> {
        let plain = format!("http://127.0.0.1:{}", self.port);
        let with_token = format!("http://dusk:{}@127.0.0.1:{}", self.token.0, self.port);
        let mut env = vec![
            ("DUSKCODE_PLUGIN_PROXY".to_owned(), plain),
            (
                "DUSKCODE_PLUGIN_PROXY_TOKEN".to_owned(),
                self.token.0.clone(),
            ),
        ];
        for name in ["HTTPS_PROXY", "HTTP_PROXY", "https_proxy", "http_proxy"] {
            env.push((name.to_owned(), with_token.clone()));
        }
        env
    }
}

/// A fresh random capability token.
pub fn new_token() -> Result<Secret> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
    Ok(Secret(
        bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}

async fn accept(listener: TcpListener, policy: Arc<RwLock<Arc<Policy>>>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let policy = Arc::clone(&policy.read());
        tokio::spawn(async move {
            if let Err(error) = serve(stream, &policy).await {
                // The error names no key or URL with one in it.
                debug!("plugin proxy: {error:#}");
            }
        });
    }
}

/// A request's head, as read.
pub(super) struct Head {
    method: String,
    target: String,
    headers: Vec<(String, Vec<u8>)>,
}

impl Head {
    fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_slice())
    }
}

async fn serve(mut stream: TcpStream, policy: &Policy) -> Result<()> {
    let (head, rest) = match tokio::time::timeout(HEAD_TIME, read_head(&mut stream)).await {
        Ok(Ok(read)) => read,
        Ok(Err(error)) => {
            respond(&mut stream, 400, "not a request").await;
            return Err(error);
        }
        Err(_) => bail!("the request head did not come in time"),
    };
    if !authorized(&head, &policy.token) {
        let answer = "HTTP/1.1 407 Proxy Authentication Required\r\n\
                      Proxy-Authenticate: Basic realm=\"duskcode\"\r\n\
                      Content-Length: 0\r\nConnection: close\r\n\r\n";
        let _ = stream.write_all(answer.as_bytes()).await;
        bail!("a request without the plugin's token");
    }
    let outcome = match head.method.as_str() {
        "CONNECT" => tunnel::open(stream, &head, rest, policy).await,
        _ => forward::send(&mut stream, &head, rest, policy).await,
    };
    outcome.map_err(|refused| anyhow::anyhow!("{}", refused.reason))
}

/// Why a request was not let through; already answered to the plugin.
pub(super) struct Refused {
    reason: String,
}

/// Answers `status` with `reason` as a short text body, then closes.
pub(super) async fn refuse(stream: &mut TcpStream, status: u16, reason: String) -> Refused {
    respond(stream, status, &reason).await;
    Refused { reason }
}

async fn respond(stream: &mut TcpStream, status: u16, text: &str) {
    let answer = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        reason_phrase(status),
        text.len()
    );
    let _ = stream.write_all(answer.as_bytes()).await;
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        403 => "Forbidden",
        411 => "Length Required",
        413 => "Content Too Large",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

/// The head, and the bytes read past it.
async fn read_head(stream: &mut TcpStream) -> Result<(Head, Vec<u8>)> {
    let mut buffer = Vec::with_capacity(4096);
    let mut chunk = [0u8; 4096];
    loop {
        let read = stream.read(&mut chunk).await?;
        ensure!(read > 0, "the connection closed before a request");
        buffer.extend_from_slice(&chunk[..read]);
        let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
        let mut request = httparse::Request::new(&mut headers);
        match request.parse(&buffer)? {
            httparse::Status::Complete(length) => {
                let head = Head {
                    method: request.method.context("no method")?.to_owned(),
                    target: request.path.context("no target")?.to_owned(),
                    headers: request
                        .headers
                        .iter()
                        .map(|header| (header.name.to_owned(), header.value.to_vec()))
                        .collect(),
                };
                return Ok((head, buffer.split_off(length)));
            }
            httparse::Status::Partial => {
                ensure!(buffer.len() <= MAX_HEAD, "the request head is too long")
            }
        }
    }
}

fn authorized(head: &Head, token: &Secret) -> bool {
    let Some(value) = head.header("proxy-authorization") else {
        return false;
    };
    let Some(encoded) = value.strip_prefix(b"Basic ") else {
        return false;
    };
    let Ok(decoded) = STANDARD.decode(encoded.trim_ascii()) else {
        return false;
    };
    let Some(colon) = decoded.iter().position(|byte| *byte == b':') else {
        return false;
    };
    same(&decoded[colon + 1..], token.0.as_bytes())
}

/// Equal, in a time that does not tell how much of it matched.
fn same(left: &[u8], right: &[u8]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// Connects to the first of `addresses` that answers.
pub(super) async fn connect(addresses: &[SocketAddr]) -> Result<TcpStream> {
    let mut last = None;
    for address in addresses {
        match tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(error)) => last = Some(anyhow::Error::from(error)),
            Err(_) => last = Some(anyhow::anyhow!("the connection timed out")),
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("no address to connect to")))
}

#[cfg(test)]
mod tests;
