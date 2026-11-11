//! Who may talk to the core: whoever holds this run's token signs in once and gets a
//! session cookie; nothing else opens the stream.

mod limiter;
mod session;

use axum::http::{HeaderMap, header};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use subtle::ConstantTimeEq;

use limiter::Limiter;
use session::Sessions;

/// The cookie's name where the browser takes the `__Host-` prefix (a `Secure` cookie on a
/// loopback page) and where it does not (a page on another address).
const SECURE_COOKIE: &str = "__Host-dusk";
const PLAIN_COOKIE: &str = "dusk";

pub struct Auth {
    token: String,
    sessions: Sessions,
    limiter: Limiter,
}

pub enum Login {
    /// Signed in; the value of `Set-Cookie`.
    Granted(String),
    WrongToken,
    /// Too many failures; the client may try again after this many seconds.
    Limited(u64),
}

impl Auth {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self {
            token: random_text()?,
            sessions: Sessions::default(),
            limiter: Limiter::default(),
        })
    }

    /// The token to show the user once; it is never put in a log.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Exchanges the token for a session. `peer` is who asks, for the limit on failures;
    /// `secure` is whether the cookie can carry the `__Host-` prefix.
    pub fn login(&self, given: &str, peer: &str, secure: bool) -> Login {
        if let Some(wait) = self.limiter.blocked(peer) {
            return Login::Limited(wait);
        }
        let right: bool = given.as_bytes().ct_eq(self.token.as_bytes()).into();
        if !right {
            self.limiter.failed(peer);
            return Login::WrongToken;
        }
        match random_text().map(|id| self.sessions.open(id)) {
            Ok(Some(id)) => Login::Granted(cookie(&id, secure)),
            _ => Login::Limited(1),
        }
    }

    /// Whether the request carries a live session.
    pub fn signed_in(&self, headers: &HeaderMap) -> bool {
        cookie_values(headers).any(|id| self.sessions.valid(id))
    }

    /// The cookie that ends the session of this request.
    pub fn logout(&self, headers: &HeaderMap, secure: bool) -> String {
        for id in cookie_values(headers) {
            self.sessions.close(id);
        }
        expired_cookie(secure)
    }
}

fn cookie_name(secure: bool) -> &'static str {
    if secure { SECURE_COOKIE } else { PLAIN_COOKIE }
}

fn cookie(id: &str, secure: bool) -> String {
    let prefix = if secure { "; Secure" } else { "" };
    format!(
        "{}={id}; Path=/; HttpOnly; SameSite=Strict{prefix}",
        cookie_name(secure)
    )
}

fn expired_cookie(secure: bool) -> String {
    let prefix = if secure { "; Secure" } else { "" };
    format!(
        "{}=; Path=/; Max-Age=0; HttpOnly; SameSite=Strict{prefix}",
        cookie_name(secure)
    )
}

/// The values of our cookies in the request, whichever name this browser could use.
fn cookie_values(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|line| line.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .filter(|(name, _)| *name == SECURE_COOKIE || *name == PLAIN_COOKIE)
        .map(|(_, value)| value)
}

/// 256 random bits as URL-safe text.
fn random_text() -> anyhow::Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|error| anyhow::anyhow!("no randomness: {error}"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests;
