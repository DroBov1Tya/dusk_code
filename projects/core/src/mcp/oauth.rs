//! Signing in to a remote MCP server with OAuth, as the MCP authorization spec has it:
//! discovery, dynamic client registration, the authorization code with PKCE in the
//! user's browser, and a redirect to a one-shot listener on 127.0.0.1. The tokens stay
//! in the config folder, which tools may not touch; the SDK refreshes them.

use std::{
    future::Future,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use reqwest::{Client, Url};
use rmcp::transport::auth::{
    AuthClient, AuthError, AuthorizationManager, AuthorizationRequest, CredentialStore, OAuthState,
    StoredCredentials,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use crate::{
    init::{create_private_dir, paths::is_plain_folder_name},
    providers::is_loopback,
};

/// How long the browser has to come back.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const MAX_REQUEST_BYTES: usize = 8 * 1024;
/// How long one connection to the redirect listener may take to say what it wants.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_NAME: &str = "duskcode";
const DONE_PAGE: &str = "<!doctype html><title>duskcode</title><p>Signed in. You can close \
                         this tab and go back to duskcode.</p>";

/// A server's tokens, kept with the URL they were issued for: a server moved to another
/// URL signs in again.
pub struct TokenFile {
    path: PathBuf,
    url: String,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    url: String,
    credentials: StoredCredentials,
}

impl TokenFile {
    /// `None` for a server name that is not a plain file name.
    pub fn new(folder: &Path, name: &str, url: &str) -> Option<Self> {
        is_plain_folder_name(name).then(|| Self {
            path: folder.join(format!("{name}.json")),
            url: url.to_owned(),
        })
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    fn write(&self, bytes: &[u8]) -> Result<()> {
        let folder = self.path.parent().context("token folder is missing")?;
        create_private_dir(folder)?;
        let temporary = self.path.with_extension("json.tmp");
        // A new file every time: create_new neither follows a link left there nor keeps
        // the mode of an old file.
        match std::fs::remove_file(&temporary) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
            _ => {}
        }
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.path)?;
        Ok(())
    }
}

fn store_error(error: impl std::fmt::Display) -> AuthError {
    AuthError::InternalError(format!("token file: {error}"))
}

#[async_trait::async_trait]
impl CredentialStore for TokenFile {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(store_error(error)),
        };
        let saved: Saved = serde_json::from_slice(&bytes).map_err(store_error)?;
        Ok((saved.url == self.url).then_some(saved.credentials))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let saved = Saved {
            url: self.url.clone(),
            credentials,
        };
        let bytes = serde_json::to_vec(&saved).map_err(store_error)?;
        self.write(&bytes).map_err(store_error)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        match tokio::fs::remove_file(&self.path).await {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(store_error(error)),
            _ => Ok(()),
        }
    }
}

/// The SDK's error with its causes, which say what went wrong. Its guard against
/// internal addresses refuses the 198.18.0.0/15 range that VPNs with fake-IP DNS hand
/// out for every name; that case is named, since nothing in it says so.
fn oauth_error(error: AuthError) -> anyhow::Error {
    let mut text = error.to_string();
    let mut cause = std::error::Error::source(&error);
    while let Some(next) = cause {
        text.push_str(&format!(": {next}"));
        cause = next.source();
    }
    if text.contains("is allowed for OAuth requests") {
        return anyhow!(
            "the sign-in server resolves to an address OAuth may not reach, such as the \
             198.18.x.x a VPN with fake-IP DNS gives; turn fake-IP off for it or reach it \
             through the VPN's HTTP proxy (HTTPS_PROXY)"
        );
    }
    anyhow!("OAuth: {text}")
}

/// A client that sends the saved token, or `None` when the server was never signed in to.
pub async fn authorized(
    tokens: TokenFile,
    url: &Url,
    http: Client,
) -> Result<Option<AuthClient<Client>>> {
    if !tokens.exists() {
        return Ok(None);
    }
    let mut manager = AuthorizationManager::new(url.as_str())
        .await
        .map_err(oauth_error)?;
    manager.set_credential_store(tokens);
    let signed_in = manager.initialize_from_store().await.map_err(oauth_error)?;
    Ok(signed_in.then(|| AuthClient::new(http, manager)))
}

/// Registers with the server's authorization server and returns the page to open in
/// the browser, and the wait for the user to come back, which saves the tokens.
pub async fn sign_in(
    tokens: TokenFile,
    url: &Url,
) -> Result<(Url, impl Future<Output = Result<()>> + Send + use<>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("cannot listen for the sign-in redirect")?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let mut state = OAuthState::new(url.as_str(), None)
        .await
        .map_err(oauth_error)?;
    if let OAuthState::Unauthorized(manager) = &mut state {
        manager.set_credential_store(tokens);
    }
    state
        .start_authorization(AuthorizationRequest::new(&redirect).with_client_name(CLIENT_NAME))
        .await
        .map_err(oauth_error)?;
    let page = state.get_authorization_url().await.map_err(oauth_error)?;
    let page = Url::parse(&page).context("the authorization page is not a URL")?;
    // The user types a password there: cleartext only to this machine.
    ensure!(
        page.scheme() == "https" && page.host_str().is_some() || is_loopback(&page),
        "the authorization page must use HTTPS"
    );
    let sent_state = page
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.into_owned())
        .context("the authorization page has no state")?;
    let finish = async move {
        let wait = redirected(&listener, port, "/callback", &sent_state);
        let callback = tokio::time::timeout(SIGN_IN_TIMEOUT, wait)
            .await
            .map_err(|_| anyhow!("the browser did not come back in time"))??;
        state
            .handle_callback_url(&callback)
            .await
            .map_err(oauth_error)?;
        Ok(())
    };
    Ok((page, finish))
}

/// The redirect URL the browser comes back with, carrying the `state` this sign-in
/// sent. Anything else, a stalled connection included, is turned away and the wait goes
/// on, so another local process cannot end it.
pub(crate) async fn redirected(
    listener: &TcpListener,
    port: u16,
    path: &str,
    state: &str,
) -> Result<String> {
    loop {
        let Ok((mut stream, _)) = listener.accept().await else {
            continue;
        };
        let Ok(Ok(head)) = tokio::time::timeout(REQUEST_TIMEOUT, read_head(&mut stream)).await
        else {
            continue;
        };
        let callback = callback_url(&head, port, path).filter(|callback| {
            callback
                .query_pairs()
                .any(|(key, value)| key == "state" && value == state)
        });
        let Some(callback) = callback else {
            let _ = stream
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await;
            continue;
        };
        let denied = callback
            .query_pairs()
            .find(|(key, _)| key == "error")
            // A code, not prose: what else the server wrote could carry anything.
            .map(|(_, value)| {
                value
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(60)
                    .collect::<String>()
            });
        let page = match &denied {
            Some(_) => "<!doctype html><p>Sign-in was not completed.</p>",
            None => DONE_PAGE,
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        if let Some(error) = denied {
            bail!("the server refused the sign-in ({error})");
        }
        return Ok(callback.into());
    }
}

async fn read_head(stream: &mut tokio::net::TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0; 1024];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await?;
        if read == 0 || head.len() + read > MAX_REQUEST_BYTES {
            break;
        }
        head.extend_from_slice(&buffer[..read]);
    }
    Ok(head)
}

/// `GET <path>?...` as a full URL on this machine.
fn callback_url(head: &[u8], port: u16, path: &str) -> Option<Url> {
    let line = String::from_utf8_lossy(head);
    let target = line
        .lines()
        .next()?
        .strip_prefix("GET ")?
        .split(' ')
        .next()
        .filter(|target| {
            target
                .strip_prefix(path)
                .is_some_and(|rest| rest.starts_with('?'))
        })?;
    Url::parse(&format!("http://127.0.0.1:{port}{target}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[tokio::test]
    async fn tokens_are_private_and_bound_to_their_url() {
        let folder = temp_dir("mcp-tokens").join("mcp-auth");
        let tokens = TokenFile::new(&folder, "notes", "https://a.example/mcp").unwrap();
        assert!(tokens.load().await.unwrap().is_none());
        tokens
            .save(StoredCredentials::new(
                "client".into(),
                None,
                Vec::new(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(tokens.load().await.unwrap().unwrap().client_id, "client");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(folder.join("notes.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }
        let moved = TokenFile::new(&folder, "notes", "https://b.example/mcp").unwrap();
        assert!(moved.load().await.unwrap().is_none());
        assert!(TokenFile::new(&folder, "../x", "https://a.example/mcp").is_none());
    }

    #[tokio::test]
    async fn only_the_callback_is_taken() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let browser = tokio::spawn(async move {
            for target in [
                "/favicon.ico",
                "/callback?code=forged&state=other",
                "/callback?code=c&state=s",
            ] {
                let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                let request = format!("GET {target} HTTP/1.1\r\nHost: x\r\n\r\n");
                stream.write_all(request.as_bytes()).await.unwrap();
                let mut answer = String::new();
                stream.read_to_string(&mut answer).await.unwrap();
            }
        });
        let callback = redirected(&listener, port, "/callback", "s").await.unwrap();
        assert_eq!(
            callback,
            format!("http://127.0.0.1:{port}/callback?code=c&state=s")
        );
        browser.await.unwrap();
    }
}
