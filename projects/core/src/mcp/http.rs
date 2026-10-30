//! A remote server over Streamable HTTP: each message is a POST to one URL, answered with
//! JSON or with an event stream that carries the answer. The server may open a session
//! when it is initialized; its ID goes with every later request.

use std::{sync::Mutex, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{ACCEPT, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue},
    redirect::Policy,
};
use serde_json::Value;

use super::lock;
use crate::{
    config::HttpServerConfig,
    providers::{SseDecoder, is_loopback},
};

const SESSION: &str = "mcp-session-id";
const PROTOCOL: &str = "mcp-protocol-version";
/// A longer answer is broken or hostile; it is not read further.
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;
/// Longest silence tolerated while a tool works and the stream stays open.
const IDLE_TIMEOUT: Duration = Duration::from_secs(300);
const READ_FAILED: &str = "cannot read the server answer";

pub struct HttpTransport {
    client: Client,
    url: Url,
    /// What the config adds to every request, such as an authorization; all secret.
    headers: HeaderMap,
    session: Mutex<Option<HeaderValue>>,
    protocol: Mutex<Option<HeaderValue>>,
}

/// HTTPS, or plain HTTP to this machine by its address, without credentials in the URL.
pub fn check_url(url: &str) -> Result<Url> {
    let url = Url::parse(url).map_err(|_| anyhow!("invalid MCP server URL"))?;
    ensure!(
        url.scheme() == "https" && url.host_str().is_some() || is_loopback(&url),
        "MCP server URL must use HTTPS, or HTTP to 127.0.0.1 or [::1]"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none() && url.fragment().is_none(),
        "MCP server URL must not contain credentials or a fragment"
    );
    Ok(url)
}

/// The config's headers as they go out, every value marked sensitive.
pub fn check_headers(config: &HttpServerConfig) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    for (name, value) in &config.headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| anyhow!("invalid MCP header name {name:?}"))?;
        let mut value = HeaderValue::from_str(value)
            .map_err(|_| anyhow!("invalid value of MCP header {name}"))?;
        value.set_sensitive(true);
        headers.insert(name, value);
    }
    Ok(headers)
}

impl HttpTransport {
    pub fn new(config: &HttpServerConfig) -> Result<Self> {
        let client = Client::builder()
            .redirect(Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(IDLE_TIMEOUT)
            .build()
            .map_err(|_| anyhow!("cannot build MCP HTTP client"))?;
        Ok(Self {
            client,
            url: check_url(&config.url)?,
            headers: check_headers(config)?,
            session: Mutex::default(),
            protocol: Mutex::default(),
        })
    }

    /// Sends a request and reads the message that answers `id`. The answer to
    /// `initialize` sets the session and protocol version for what follows.
    pub async fn request(&self, id: u64, message: &Value) -> Result<Value> {
        let response = self.post(message).await?;
        let initializing = message.get("method").and_then(Value::as_str) == Some("initialize");
        if initializing && let Some(session) = response.headers().get(SESSION) {
            *lock(&self.session) = Some(session.clone());
        }
        let stream = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        let answer = if stream {
            read_stream(response, id).await?
        } else {
            let body = read_body(response).await?;
            serde_json::from_slice(&body).context("the server answered with invalid JSON")?
        };
        if initializing
            && let Some(version) = answer
                .pointer("/result/protocolVersion")
                .and_then(Value::as_str)
        {
            *lock(&self.protocol) = HeaderValue::from_str(version).ok();
        }
        Ok(answer)
    }

    /// A notification is accepted without an answer.
    pub async fn send(&self, message: &Value) -> Result<()> {
        self.post(message).await.map(drop)
    }

    async fn post(&self, message: &Value) -> Result<Response> {
        let mut request = self
            .client
            .post(self.url.clone())
            .headers(self.headers.clone())
            .header(ACCEPT, "application/json, text/event-stream")
            .header(CONTENT_TYPE, "application/json")
            .body(serde_json::to_vec(message)?);
        if let Some(session) = lock(&self.session).clone() {
            request = request.header(SESSION, session);
        }
        if let Some(protocol) = lock(&self.protocol).clone() {
            request = request.header(PROTOCOL, protocol);
        }
        // The error text can name the URL, which may carry a key in its query.
        let response = request
            .send()
            .await
            .map_err(|_| anyhow!("cannot reach the server"))?;
        match response.status() {
            status if status.is_success() => Ok(response),
            StatusCode::NOT_FOUND if lock(&self.session).is_some() => {
                bail!("the server ended the session")
            }
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                bail!("the server refused access ({})", response.status())
            }
            status => bail!("the server answered {status}"),
        }
    }
}

async fn read_body(mut response: Response) -> Result<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!(READ_FAILED))? {
        ensure!(
            chunk.len() <= MAX_ANSWER_BYTES - body.len(),
            "the server answer is over {MAX_ANSWER_BYTES} bytes"
        );
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Reads events until one answers `id`. Notifications and requests of the server on the
/// way are passed over.
async fn read_stream(mut response: Response, id: u64) -> Result<Value> {
    let mut decoder = SseDecoder::default();
    let mut read = 0;
    while let Some(chunk) = response.chunk().await.map_err(|_| anyhow!(READ_FAILED))? {
        read += chunk.len();
        ensure!(
            read <= MAX_ANSWER_BYTES,
            "the server stream is over {MAX_ANSWER_BYTES} bytes"
        );
        for data in decoder.feed(&chunk)? {
            let Ok(message) = serde_json::from_str::<Value>(&data) else {
                continue;
            };
            if super::answer_id(&message) == Some(id) {
                return Ok(message);
            }
        }
    }
    bail!("the server closed the stream without an answer")
}
