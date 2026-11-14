//! Signing in with a ChatGPT account, the way OpenAI offers it to open-source and local
//! apps: the browser asks the user, OpenAI issues this install a client ID of its own,
//! and requests then run on the user's plan. There is no API key and no client secret.
//!
//! What is kept is one record: the issued client ID, the host ID of this install and the
//! tokens. The refresh token rotates, so one account has one live record in the process,
//! shared by every adapter built for it, and each refresh is handed to whoever saves it.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, LazyLock, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use aws_lc_rs::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;

use crate::mcp::oauth::redirected;

const ISSUER: &str = "https://auth.openai.com";
const AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const KEYS: &str = "https://auth.openai.com/.well-known/jwks.json";
/// The API the tokens are for.
pub const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// Without this scope the account is known but its plan may not be used.
const PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
/// The client ID of a first sign-in; OpenAI answers it with the one it issues.
const FIRST_CLIENT: &str = "dynamic_agent_client";
const APP_NAME: &str = "duskcode";
const CALLBACK: &str = "/auth/callback";
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(300);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_ANSWER_BYTES: usize = 256 * 1024;
/// A token this close to its end is renewed before it is used.
const RENEW_BEFORE: i64 = 120;
/// Refresh answers that mean the refresh token is of no use any more.
const SIGNED_OUT: [&str; 6] = [
    "invalid_grant",
    "invalid_refresh_token",
    "token_expired",
    "refresh_token_expired",
    "refresh_token_invalidated",
    "refresh_token_reused",
];

/// What a sign-in leaves, kept where a provider's API key is kept. It has no `Debug`:
/// nothing may print the tokens.
#[derive(Clone, Serialize, Deserialize)]
pub struct Credential {
    pub client_id: String,
    /// Names this install to OpenAI; made once and sent with every sign-in.
    pub host_id: String,
    /// The account, as OpenAI names it.
    pub subject: String,
    pub email: Option<String>,
    access_token: String,
    refresh_token: String,
    /// Unix seconds.
    expires_at: i64,
    saved_at: i64,
}

impl Credential {
    pub fn read(record: &str) -> Result<Self> {
        serde_json::from_str(record)
            .map_err(|_| anyhow!("the ChatGPT sign-in record is damaged; sign in again"))
    }

    pub fn record(&self) -> String {
        serde_json::to_string(self).expect("a credential is plain text fields")
    }
}

/// One account's tokens, shared by every adapter built from its record.
pub struct Session {
    tokens: tokio::sync::Mutex<Held>,
    /// A record that came while a renewal held the tokens, as from a new sign-in; it is
    /// taken up before the tokens are used again.
    newer: parking_lot::Mutex<Option<Credential>>,
}

struct Held {
    credential: Credential,
    /// The last renewal could not be kept; the next use tries again, since the record
    /// in the store holds a refresh token that is spent.
    unsaved: bool,
}

static SESSIONS: LazyLock<parking_lot::Mutex<HashMap<String, Arc<Session>>>> =
    LazyLock::new(Default::default);

type Load = Box<dyn Fn(&str) -> Option<Credential> + Send + Sync>;
type Save = Box<dyn Fn(&Credential) -> Result<()> + Send + Sync>;

/// Where the record of an account is kept: another process of this program may have
/// renewed it since, so it is read again before a renewal, and written after one.
pub struct Store {
    /// The stored record of the account with this client ID.
    pub load: Load,
    pub save: Save,
}

static STORE: OnceLock<Store> = OnceLock::new();

/// Set once, by the runtime that owns the config.
pub fn keep_in(store: Store) {
    let _ = STORE.set(store);
}

impl Session {
    /// The session of `record`'s account. A record newer than the one in memory, as after
    /// a new sign-in, takes its place.
    pub fn of(record: &str) -> Result<Arc<Self>> {
        let credential = Credential::read(record)?;
        let mut sessions = SESSIONS.lock();
        let session = sessions
            .entry(credential.client_id.clone())
            .or_insert_with(|| {
                Arc::new(Self {
                    tokens: tokio::sync::Mutex::new(Held {
                        credential: credential.clone(),
                        unsaved: false,
                    }),
                    newer: Default::default(),
                })
            });
        *session.newer.lock() = Some(credential);
        Ok(Arc::clone(session))
    }

    /// A token good for a request now. It is renewed first when it is about to end, or
    /// when the server refused `refused`, unless someone has renewed it meanwhile.
    pub async fn access_token(&self, http: &Client, refused: Option<&str>) -> Result<String> {
        let mut held = self.tokens.lock().await;
        if let Some(newer) = self.newer.lock().take() {
            held.adopt(newer);
        }
        let stale = |held: &Held| {
            refused == Some(held.credential.access_token.as_str())
                || held.credential.expires_at.saturating_sub(now()) < RENEW_BEFORE
        };
        // Another process of this program may hold a renewed record already: using the
        // refresh token it spent would end the sign-in for both.
        if stale(&held)
            && let Some(stored) = STORE
                .get()
                .and_then(|store| (store.load)(&held.credential.client_id))
        {
            held.adopt(stored);
        }
        if stale(&held) {
            held.credential = refresh(http, &held.credential).await?;
            held.unsaved = true;
        }
        if held.unsaved
            && let Some(store) = STORE.get()
        {
            match (store.save)(&held.credential) {
                Ok(()) => held.unsaved = false,
                Err(error) => {
                    tracing::warn!("the renewed ChatGPT sign-in is not saved yet: {error:#}")
                }
            }
        }
        Ok(held.credential.access_token.clone())
    }
}

impl Held {
    /// Takes `other` when it was made later. A renewal not yet saved is the latest there
    /// is, whatever a clock says.
    fn adopt(&mut self, other: Credential) {
        if !self.unsaved && other.saved_at >= self.credential.saved_at {
            self.credential = other;
        }
    }
}

#[derive(Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: Option<String>,
    id_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
}

#[derive(Deserialize)]
struct Refused {
    error: Option<serde_json::Value>,
}

/// The page to open in the browser, and the wait for the user to come back, which gives
/// the record to keep. With `earlier`, the same client and host sign in again.
pub async fn sign_in(
    earlier: Option<Credential>,
) -> Result<(Url, impl Future<Output = Result<Credential>> + Send)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("cannot listen for the sign-in redirect")?;
    let port = listener.local_addr()?.port();
    let redirect = format!("http://127.0.0.1:{port}{CALLBACK}");
    let (state, nonce, verifier) = (random()?, random()?, random()?);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let host_id = match &earlier {
        Some(earlier) => earlier.host_id.clone(),
        None => format!("urn:uuid:{}", uuid::Uuid::new_v4()),
    };
    let client_id = earlier
        .as_ref()
        .map_or(FIRST_CLIENT, |earlier| earlier.client_id.as_str());
    let mut page = Url::parse(AUTHORIZE)?;
    {
        let mut query = page.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", client_id)
            .append_pair("redirect_uri", &redirect)
            .append_pair("scope", SCOPE)
            .append_pair("resource", RESOURCE)
            .append_pair("state", &state)
            .append_pair("nonce", &nonce)
            .append_pair("code_challenge_method", "S256")
            .append_pair("code_challenge", &challenge)
            .append_pair("ext_agent_host_id", &host_id);
        if earlier.is_none() {
            query.append_pair("agent_name_hint", APP_NAME);
        }
    }
    let finish = async move {
        let wait = redirected(&listener, port, CALLBACK, &state);
        let callback = tokio::time::timeout(SIGN_IN_TIMEOUT, wait)
            .await
            .map_err(|_| anyhow!("the browser did not come back in time"))??;
        let callback = Url::parse(&callback)?;
        let value = |name: &str| {
            callback
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        let code = value("code").context("the sign-in came back without a code")?;
        let client_id = issued_client(earlier.as_ref(), value("client_id"))?;
        let http = crate::providers::transport::http::client()?;
        let tokens = ask_tokens(
            &http,
            &[
                ("grant_type", "authorization_code"),
                ("client_id", &client_id),
                ("code", &code),
                ("code_verifier", &verifier),
                ("redirect_uri", &redirect),
                ("resource", RESOURCE),
            ],
        )
        .await?;
        let id_token = tokens
            .id_token
            .as_deref()
            .context("the sign-in gave no identity")?;
        let identity = verified(&http, id_token, &client_id, &nonce).await?;
        if let Some(earlier) = &earlier {
            ensure!(
                earlier.subject == identity.sub,
                "this is another ChatGPT account; remove the provider to change accounts"
            );
        }
        ensure!(
            granted(tokens.scope.as_deref()),
            "the account signed in, but use of its ChatGPT plan was not allowed"
        );
        Ok(Credential {
            client_id,
            host_id,
            subject: identity.sub,
            email: identity.email,
            access_token: tokens.access_token,
            refresh_token: tokens
                .refresh_token
                .context("the sign-in gave no refresh token")?,
            expires_at: expiry(tokens.expires_in),
            saved_at: now(),
        })
    };
    Ok((page, finish))
}

/// The client ID to go on with. A first sign-in must be answered with an issued one; a
/// later one may repeat its own and nothing else.
fn issued_client(earlier: Option<&Credential>, answered: Option<String>) -> Result<String> {
    match (earlier, answered) {
        (None, Some(issued)) if issued != FIRST_CLIENT && !issued.is_empty() => Ok(issued),
        (None, _) => bail!("the sign-in did not issue a client ID"),
        (Some(earlier), Some(other)) if other != earlier.client_id => {
            bail!("the sign-in answered for another client")
        }
        (Some(earlier), _) => Ok(earlier.client_id.clone()),
    }
}

/// When a token ends, from the seconds the server gave: an hour when it gave none, and
/// never an absurd span.
fn expiry(expires_in: Option<i64>) -> i64 {
    now() + expires_in.unwrap_or(3600).clamp(0, 30 * 24 * 3600)
}

fn granted(scope: Option<&str>) -> bool {
    scope.is_some_and(|scope| scope.split_whitespace().any(|one| one == PLAN_SCOPE))
}

async fn refresh(http: &Client, credential: &Credential) -> Result<Credential> {
    let tokens = ask_tokens(
        http,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", &credential.client_id),
            ("refresh_token", &credential.refresh_token),
            ("resource", RESOURCE),
        ],
    )
    .await?;
    Ok(Credential {
        access_token: tokens.access_token,
        // It rotates; an answer without one keeps the old.
        refresh_token: tokens
            .refresh_token
            .unwrap_or_else(|| credential.refresh_token.clone()),
        expires_at: expiry(tokens.expires_in),
        // Later than the record it came from even within one second, or after the
        // clock stepped back.
        saved_at: now().max(credential.saved_at + 1),
        ..credential.clone()
    })
}

/// Asks the token endpoint. Its answers can echo what was sent, so an error names only
/// the code.
async fn ask_tokens(http: &Client, form: &[(&str, &str)]) -> Result<Tokens> {
    // The query of a URL is the same encoding as a form's body.
    let encoded = Url::parse_with_params("http://form.invalid/", form)?
        .query()
        .unwrap_or_default()
        .to_owned();
    let response = http
        .post(TOKEN)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(encoded)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        // The network may be back in a moment; a turn waits and asks again.
        .map_err(|_| {
            crate::providers::Transient("cannot reach the ChatGPT sign-in server".into())
        })?;
    let status = response.status();
    let body = limited(response).await?;
    if status.is_success() {
        return serde_json::from_slice(&body)
            .map_err(|_| anyhow!("the ChatGPT sign-in server gave an answer that is not tokens"));
    }
    let code = serde_json::from_slice::<Refused>(&body)
        .ok()
        .and_then(|refused| refused.error)
        .and_then(|error| match error {
            serde_json::Value::String(code) => Some(code),
            other => other.get("code")?.as_str().map(str::to_owned),
        })
        .unwrap_or_default();
    if SIGNED_OUT.contains(&code.as_str()) {
        bail!("the ChatGPT sign-in has ended; sign in again");
    }
    let code: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(60)
        .collect();
    bail!("the ChatGPT sign-in server refused ({status} {code})")
}

async fn limited(mut response: reqwest::Response) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| anyhow!("cannot read the ChatGPT sign-in server's answer"))?
    {
        ensure!(
            chunk.len() <= MAX_ANSWER_BYTES - bytes.len(),
            "the ChatGPT sign-in server's answer is too large"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct Identity {
    iss: String,
    aud: Audience,
    /// The party the token was issued to; stated when there are several audiences.
    azp: Option<String>,
    exp: i64,
    nonce: Option<String>,
    sub: String,
    email: Option<String>,
}

/// OpenID Connect Core 3.1.3.7: the client is one of the audiences, and the authorized
/// party, when one is stated, is the client.
fn for_client(aud: &Audience, azp: Option<&str>, client_id: &str) -> bool {
    let among = match aud {
        Audience::One(one) => one == client_id,
        Audience::Many(many) => many.iter().any(|one| one == client_id),
    };
    among && azp.is_none_or(|azp| azp == client_id)
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize)]
struct TokenHead {
    alg: String,
    kid: Option<String>,
}

#[derive(Deserialize)]
struct KeySet {
    keys: Vec<Key>,
}

#[derive(Deserialize)]
struct Key {
    kty: String,
    kid: Option<String>,
    n: Option<String>,
    e: Option<String>,
}

/// The identity an ID token states, once its signature is OpenAI's and it was made for
/// this client, this sign-in and this moment.
async fn verified(http: &Client, token: &str, client_id: &str, nonce: &str) -> Result<Identity> {
    let response = http
        .get(KEYS)
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|_| anyhow!("cannot fetch the keys the sign-in is checked with"))?;
    ensure!(
        response.status().is_success(),
        "the keys the sign-in is checked with were refused"
    );
    let keys: KeySet = serde_json::from_slice(&limited(response).await?)
        .map_err(|_| anyhow!("the keys the sign-in is checked with are not keys"))?;
    check_identity(token, &keys, client_id, nonce, now())
}

fn check_identity(
    token: &str,
    keys: &KeySet,
    client_id: &str,
    nonce: &str,
    now: i64,
) -> Result<Identity> {
    let unsigned = || anyhow!("the sign-in's identity is not a signed token");
    let mut parts = token.split('.');
    let (Some(head), Some(body), Some(signature), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(unsigned());
    };
    let decode = |part: &str| URL_SAFE_NO_PAD.decode(part).map_err(|_| unsigned());
    let stated: TokenHead = serde_json::from_slice(&decode(head)?).map_err(|_| unsigned())?;
    // One algorithm, named here: the token does not choose how it is checked.
    ensure!(
        stated.alg == "RS256",
        "the sign-in's identity is signed in an unexpected way"
    );
    let key = keys
        .keys
        .iter()
        .find(|key| key.kty == "RSA" && key.kid.is_some() && key.kid == stated.kid)
        .context("the sign-in's identity is signed with an unknown key")?;
    let (n, e) = (
        decode(key.n.as_deref().unwrap_or_default())?,
        decode(key.e.as_deref().unwrap_or_default())?,
    );
    let signed = &token[..head.len() + 1 + body.len()];
    RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &decode(signature)?,
        )
        .map_err(|_| anyhow!("the sign-in's identity has a wrong signature"))?;
    let identity: Identity = serde_json::from_slice(&decode(body)?).map_err(|_| unsigned())?;
    ensure!(
        identity.iss == ISSUER,
        "the sign-in's identity was made by someone other than OpenAI"
    );
    if !for_client(&identity.aud, identity.azp.as_deref(), client_id) {
        // Client IDs name an app's registration, not a person; they say what went wrong.
        tracing::warn!(
            expected = client_id,
            aud = ?identity.aud,
            azp = ?identity.azp,
            "the ChatGPT sign-in's identity is for another client"
        );
        anyhow::bail!("the sign-in's identity was made for another app");
    }
    ensure!(identity.exp > now, "the sign-in's identity has expired");
    ensure!(
        identity.nonce.as_deref() == Some(nonce),
        "the sign-in's identity belongs to another sign-in"
    );
    ensure!(
        !identity.sub.is_empty(),
        "the sign-in's identity names nobody"
    );
    Ok(identity)
}

/// 32 bytes from the system, as text for a URL.
fn random() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| anyhow!("the system gave no random bytes"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(json: &str) -> String {
        URL_SAFE_NO_PAD.encode(json)
    }

    fn keys() -> KeySet {
        KeySet {
            keys: vec![Key {
                kty: "RSA".into(),
                kid: Some("k1".into()),
                n: Some(part("not a real modulus")),
                e: Some("AQAB".into()),
            }],
        }
    }

    /// The checks that stand before the signature: a token cannot pick its algorithm or
    /// an unknown key, and a forged signature is refused.
    #[test]
    fn an_identity_that_is_not_openais_is_refused() {
        let body = part(r#"{"iss":"https://auth.openai.com","aud":"c","exp":9,"sub":"u"}"#);
        let token = |head: &str| format!("{}.{body}.{}", part(head), part("sig"));
        let refused = |token: String| check_identity(&token, &keys(), "c", "n", 0).is_err();
        assert!(refused(token(r#"{"alg":"none","kid":"k1"}"#)));
        assert!(refused(token(r#"{"alg":"HS256","kid":"k1"}"#)));
        assert!(refused(token(r#"{"alg":"RS256","kid":"other"}"#)));
        assert!(refused(token(r#"{"alg":"RS256"}"#)));
        assert!(refused(token(r#"{"alg":"RS256","kid":"k1"}"#)));
        assert!(refused("a.b".into()));
        assert!(refused("a.b.c.d".into()));
    }

    #[test]
    fn an_identity_is_for_this_client_only() {
        let many = |list: &[&str]| Audience::Many(list.iter().map(|one| one.to_string()).collect());
        assert!(for_client(&Audience::One("c".into()), None, "c"));
        assert!(for_client(&many(&["c", "api"]), None, "c"));
        assert!(for_client(&many(&["c", "api"]), Some("c"), "c"));
        assert!(!for_client(&Audience::One("other".into()), None, "c"));
        assert!(!for_client(&many(&["other", "api"]), None, "c"));
        assert!(!for_client(&many(&["c", "api"]), Some("other"), "c"));
        assert!(!for_client(&Audience::One("c".into()), Some("other"), "c"));
    }

    fn credential(saved_at: i64, access_token: &str) -> Credential {
        Credential {
            client_id: "oaiapp_1".into(),
            host_id: "urn:uuid:x".into(),
            subject: "u".into(),
            email: None,
            access_token: access_token.into(),
            refresh_token: "r".into(),
            expires_at: 0,
            saved_at,
        }
    }

    /// The refresh token rotates: the record made last wins, and a renewal that is not
    /// saved yet is never given up for what the store still holds.
    #[test]
    fn the_latest_record_is_kept_and_an_unsaved_renewal_is_not_replaced() {
        let mut held = Held {
            credential: credential(10, "mine"),
            unsaved: false,
        };
        held.adopt(credential(9, "older"));
        assert_eq!(held.credential.access_token, "mine");
        held.adopt(credential(11, "from another process"));
        assert_eq!(held.credential.access_token, "from another process");
        held.unsaved = true;
        held.adopt(credential(99, "stale store"));
        assert_eq!(held.credential.access_token, "from another process");
    }

    #[test]
    fn a_first_sign_in_needs_an_issued_client_and_a_later_one_keeps_its_own() {
        let earlier = Credential {
            client_id: "oaiapp_1".into(),
            host_id: "urn:uuid:x".into(),
            subject: "u".into(),
            email: None,
            access_token: "a".into(),
            refresh_token: "r".into(),
            expires_at: 0,
            saved_at: 0,
        };
        assert!(issued_client(None, None).is_err());
        assert!(issued_client(None, Some(FIRST_CLIENT.into())).is_err());
        assert_eq!(
            issued_client(None, Some("oaiapp_2".into())).unwrap(),
            "oaiapp_2"
        );
        assert_eq!(issued_client(Some(&earlier), None).unwrap(), "oaiapp_1");
        assert!(issued_client(Some(&earlier), Some("oaiapp_2".into())).is_err());
        assert!(granted(Some("openid chatgpt.tokens.use.direct")));
        assert!(!granted(Some("openid email")));
        assert!(!granted(None));
    }
}
