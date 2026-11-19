//! What every provider's HTTP client shares: where it may connect, how it is built, its
//! endpoints, and which failures may pass if the request is sent again.

use std::{collections::BTreeMap, time::Duration};

use anyhow::{Result, anyhow, ensure};
use reqwest::{
    Client, Response, StatusCode, Url,
    header::{CONTENT_TYPE, HeaderValue},
    redirect::Policy,
};

use dusk_protocol::model::CheckFailure;

use crate::providers::{
    errors::{ProviderError, ProviderErrorKind},
    registry::Model,
};

/// Longest silence tolerated between streamed chunks; reasoning models can pause for a while.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub const EVENT_STREAM: &str = "text/event-stream";

/// The most of a failed request's body read for its message.
const MAX_ERROR_BYTES: usize = 64 * 1024;
/// Error messages reach the transcript; a long one is cut.
const MAX_ERROR_CHARS: usize = 300;

/// How long the body of a failed request is waited for.
const ERROR_BODY_TIMEOUT: Duration = Duration::from_secs(5);

/// The API's own words about a failed request, at `/error/message` of its JSON body,
/// read with a cap and cut short; none when the body is too large or says nothing.
async fn error_message(response: Response) -> Option<String> {
    let bytes = crate::net::read_limited(response, MAX_ERROR_BYTES)
        .await
        .ok()?;
    let body: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let message = body.pointer("/error/message")?.as_str()?;
    Some(message.chars().take(MAX_ERROR_CHARS).collect())
}

/// A request to `what`, such as "completion", answered with a failed status: the
/// failure, its message carrying the API's own words when they came in time, and those
/// words, for the caller's checks of what the provider could not do.
pub async fn refused_with_reason(
    response: Response,
    what: &str,
) -> (ProviderError, Option<String>) {
    let status = response.status();
    // Read before the body, which takes the response.
    let mut failure = status_failure(&response, String::new());
    let reason = tokio::time::timeout(ERROR_BODY_TIMEOUT, error_message(response))
        .await
        .ok()
        .flatten();
    let refused = refused(what, status);
    failure.message = match &reason {
        Some(reason) => format!("{refused}: {reason}"),
        None => refused,
    };
    (failure, reason)
}

/// An API key as a header value, after `prefix` such as "Bearer ", marked sensitive so
/// it never shows in a log; none when there is no key.
pub fn sensitive_header(key: &str, prefix: &str) -> Result<Option<HeaderValue>> {
    if key.is_empty() {
        return Ok(None);
    }
    let mut value = HeaderValue::from_str(&format!("{prefix}{key}"))
        .map_err(|_| anyhow!("invalid provider API key"))?;
    value.set_sensitive(true);
    Ok(Some(value))
}

/// A model ID from a catalogue: not empty and with no control characters.
pub fn check_model_id(id: &str) -> Result<()> {
    ensure!(
        !id.trim().is_empty() && !id.chars().any(char::is_control),
        "provider returned an empty or invalid model ID"
    );
    Ok(())
}

/// The models of a catalogue, by ID, with their context window and output limit.
pub fn models(catalogue: BTreeMap<String, (Option<u64>, Option<u64>)>) -> Vec<Model> {
    catalogue
        .into_iter()
        .map(|(id, (context_window, max_output))| Model {
            id,
            context_window,
            max_output,
        })
        .collect()
}

/// Plain HTTP is allowed only to a server on this machine, such as a local model, named
/// by its address: a name could be made to resolve elsewhere.
pub fn is_loopback(url: &Url) -> bool {
    let address = url
        .host_str()
        .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<std::net::IpAddr>().ok());
    url.scheme() == "http" && address.is_some_and(|address| address.is_loopback())
}

/// Plain HTTP to an address of a private network, such as a model server on the LAN
/// or a Tailscale node, named by its address for the same reason as above.
pub fn is_private(url: &Url) -> bool {
    use std::net::IpAddr;
    let address = url
        .host_str()
        .map(|host| host.trim_start_matches('[').trim_end_matches(']'))
        .and_then(|host| host.parse::<IpAddr>().ok());
    let private = match address {
        Some(IpAddr::V4(v4)) => {
            let [a, b, ..] = v4.octets();
            v4.is_private() || v4.is_link_local() || a == 100 && (64..128).contains(&b)
        }
        Some(IpAddr::V6(v6)) => (v6.segments()[0] & 0xfe00) == 0xfc00,
        None => false,
    };
    url.scheme() == "http" && private
}

/// The configured base URL: HTTPS, or HTTP to this machine or a private network, or to
/// any host when `allow_http` says so in the config, since the key then travels
/// unencrypted. No credentials, query or fragment in it.
pub fn base_url(text: &str, allow_http: bool) -> Result<Url> {
    let url = Url::parse(text).map_err(|_| anyhow!("invalid provider base URL"))?;
    let plain = url.scheme() == "http" && url.host_str().is_some();
    ensure!(
        url.scheme() == "https" && url.host_str().is_some()
            || is_loopback(&url)
            || is_private(&url)
            || allow_http && plain,
        "provider base URL must use HTTPS, or HTTP to this machine or a private network; \
         set allow_http: true on the provider to send its key over plain HTTP elsewhere"
    );
    if plain && !is_loopback(&url) {
        tracing::warn!(target: "dusk_core::security", "a provider is reached over plain HTTP");
    }
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "provider base URL must not contain credentials, a query or a fragment"
    );
    Ok(url)
}

/// No redirects, so a key never follows one; no total timeout, which would cut long
/// streams. Requests bound their own duration.
pub fn client() -> Result<Client> {
    Client::builder()
        .redirect(Policy::none())
        .retry(reqwest::retry::never())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(STREAM_IDLE_TIMEOUT)
        .build()
        .map_err(|_| anyhow!("cannot build provider HTTP client"))
}

pub fn endpoint(base_url: &Url, segments: &[&str]) -> Result<Url> {
    let mut url = base_url.clone();
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid provider base URL path"))?
        .pop_if_empty()
        .extend(segments);
    Ok(url)
}

/// What a failed status says about the request. Timeouts, conflicts, rate limits,
/// overload and server errors may pass on a later attempt; the rest are the request's own.
pub fn status_kind(status: StatusCode) -> ProviderErrorKind {
    use ProviderErrorKind::*;
    match status.as_u16() {
        401 | 403 => Auth,
        402 => Quota,
        408 => TimedOut,
        // Another request on the same thing is in flight; once it ends, this one passes.
        409 => Overloaded,
        413 => TooLong,
        429 => RateLimited,
        500..=599 => Overloaded,
        _ => Refused,
    }
}

/// A request the provider answered with a failed status, carrying its `retry-after` in
/// seconds when it gave one, capped so a bad header cannot stall the turn.
pub fn status_failure(response: &Response, message: String) -> ProviderError {
    const MAX_WAIT: Duration = Duration::from_secs(60);
    let status = response.status();
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds).min(MAX_WAIT));
    ProviderError {
        status: Some(status.as_u16()),
        retry_after,
        ..ProviderError::new(status_kind(status), message)
    }
}

/// A request that got no answer. Only the host is named: a reqwest error prints its URL,
/// whose path and query may carry a key.
pub fn unsent(error: reqwest::Error) -> ProviderError {
    use ProviderErrorKind::*;
    let host = error
        .url()
        .and_then(Url::host_str)
        .unwrap_or("the provider")
        .to_owned();
    let error = error.without_url();
    let (kind, message) = if error.is_dns() {
        (Unreachable, format!("could not resolve {host}"))
    } else if error.is_connect() {
        (
            Unreachable,
            format!("could not reach {host}: {}", reason(&error)),
        )
    } else if error.is_timeout() {
        (TimedOut, format!("{host} did not answer in time"))
    } else {
        let reason = reason(&error);
        (
            Unreachable,
            format!("the connection to {host} failed: {reason}"),
        )
    };
    ProviderError {
        cause: Some(chain(&error)),
        ..ProviderError::new(kind, message)
    }
}

/// A stream that broke after the answer began.
pub fn cut(error: reqwest::Error) -> ProviderError {
    let error = error.without_url();
    let kind = match error.is_timeout() {
        true => ProviderErrorKind::TimedOut,
        false => ProviderErrorKind::Interrupted,
    };
    let message = format!("cannot read provider stream: {}", reason(&error));
    ProviderError {
        cause: Some(chain(&error)),
        ..ProviderError::new(kind, message)
    }
}

/// The innermost cause in a few words, such as "connection refused". Nested reqwest
/// errors are skipped, since they may still hold a URL.
fn reason(error: &reqwest::Error) -> String {
    use std::error::Error;
    let mut innermost: Option<&(dyn Error + 'static)> = None;
    let mut source = error.source();
    while let Some(next) = source {
        if !next.is::<reqwest::Error>() {
            innermost = Some(next);
        }
        source = next.source();
    }
    let text = match innermost {
        // An error of the system says the same as its kind, which reads better.
        Some(cause) => match cause.downcast_ref::<std::io::Error>() {
            Some(io) if io.raw_os_error().is_some() => io.kind().to_string(),
            _ => cause.to_string(),
        },
        None => error.to_string(),
    };
    printable(&text, 160)
}

/// The whole chain for the log, under the same rule as `reason`.
fn chain(error: &reqwest::Error) -> String {
    use std::error::Error;
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(next) = source {
        if !next.is::<reqwest::Error>() {
            text.push_str(": ");
            text.push_str(&next.to_string());
        }
        source = next.source();
    }
    printable(&text, 500)
}

fn printable(text: &str, limit: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(limit)
        .collect()
}

/// What a refused request tells the user; a bad key says so, with where to fix it.
/// A model list that failed in a way a client can name; its text is the error's own.
#[derive(Debug)]
pub struct CheckFailed(pub CheckFailure, pub String);

impl std::fmt::Display for CheckFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

impl std::error::Error for CheckFailed {}

pub fn check_failed(cause: CheckFailure, text: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CheckFailed(cause, text.into()))
}

/// A model list the server refused: 401 and 403 are the key, 404 a wrong address.
pub fn catalogue_refused(status: StatusCode) -> anyhow::Error {
    let text = refused("model", status);
    match status.as_u16() {
        401 | 403 => check_failed(CheckFailure::Refused, text),
        404 => check_failed(CheckFailure::NotCompatible, text),
        _ => anyhow!(text),
    }
}

pub fn refused(what: &str, status: StatusCode) -> String {
    let code = status.as_u16();
    match code {
        401 => "the provider rejected the API key (HTTP 401); /connect sets another".into(),
        402 => "the provider account has no credit left (HTTP 402)".into(),
        403 => "the API key may not do this (HTTP 403)".into(),
        404 => "the provider has no such model or address (HTTP 404)".into(),
        _ => format!("provider {what} request returned HTTP {code}"),
    }
}

pub fn is_event_stream(response: &Response) -> bool {
    response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with(EVENT_STREAM))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_http_reaches_this_machine_and_private_networks() {
        let url = |text: &str| Url::parse(text).unwrap();
        assert!(is_loopback(&url("http://127.0.0.1:8080/v1")));
        assert!(is_loopback(&url("http://[::1]:8080/v1")));
        for private in [
            "http://10.0.0.1/v1",
            "http://192.168.1.20:8080/v1",
            "http://100.101.1.2/v1",
            "http://[fd00::1]/v1",
        ] {
            assert!(base_url(private, false).is_ok(), "{private}");
        }
        // Names may resolve anywhere, and public addresses are the open internet.
        for public in [
            "http://localhost/v1",
            "http://example.com/v1",
            "http://8.8.8.8/v1",
        ] {
            assert!(base_url(public, false).is_err(), "{public}");
            assert!(base_url(public, true).is_ok(), "{public}");
        }
        assert!(base_url("https://user:pass@api.example.com/v1", false).is_err());
        assert!(base_url("http://user:pass@10.0.0.1/v1", true).is_err());
        assert!(base_url("https://api.example.com/v1", false).is_ok());
    }

    /// The retry decision by status before the failures had kinds: the oracle the
    /// kinds must match.
    fn is_retryable(status: StatusCode) -> bool {
        matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
    }

    fn retried(code: u16) -> bool {
        status_kind(StatusCode::from_u16(code).unwrap()).is_retryable()
    }

    #[test]
    fn only_passing_failures_are_transient() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(StatusCode::BAD_REQUEST));
        assert!(retried(429) && retried(502) && !retried(401) && !retried(400));
    }

    #[test]
    fn statuses_are_retried_exactly_as_before() {
        for code in 300..=599 {
            let status = StatusCode::from_u16(code).unwrap();
            assert_eq!(retried(code), is_retryable(status), "HTTP {code}");
        }
    }

    #[test]
    fn statuses_have_their_kinds() {
        use ProviderErrorKind::*;
        let table = [
            (400, Refused, false),
            (401, Auth, false),
            (402, Quota, false),
            (403, Auth, false),
            (404, Refused, false),
            (408, TimedOut, true),
            (409, Overloaded, true),
            (413, TooLong, false),
            (422, Refused, false),
            (429, RateLimited, true),
            (500, Overloaded, true),
            (503, Overloaded, true),
            (529, Overloaded, true),
        ];
        for (code, kind, retryable) in table {
            let status = StatusCode::from_u16(code).unwrap();
            assert_eq!(status_kind(status), kind, "HTTP {code}");
            assert_eq!(kind.is_retryable(), retryable, "HTTP {code}");
        }
    }

    #[tokio::test]
    async fn a_failed_request_names_the_host_and_nothing_else() {
        // A port that was just free refuses the connection without leaving the machine.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/v1/secret-path?key=sk-fake-0123456789");
        let error = client()
            .unwrap()
            .post(url)
            .header("x-api-key", "sk-fake-0123456789")
            .body("user text that is private")
            .send()
            .await
            .unwrap_err();
        assert!(
            format!("{error}").contains("sk-fake"),
            "the test needs a URL in it"
        );
        let failure = unsent(error);
        assert_eq!(failure.kind, ProviderErrorKind::Unreachable);
        assert!(
            failure.message.contains("could not reach 127.0.0.1"),
            "{failure}"
        );
        let shown = anyhow::Error::new(failure);
        for text in [format!("{shown:#}"), format!("{shown:?}")] {
            for secret in ["sk-fake", "key=", "secret-path", "/v1", "private"] {
                assert!(!text.contains(secret), "{secret} in {text}");
            }
        }
    }
}
