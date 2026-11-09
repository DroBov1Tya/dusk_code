//! What every provider's HTTP client shares: where it may connect, how it is built, its
//! endpoints, and which failures may pass if the request is sent again.

use std::time::Duration;

use anyhow::{Result, anyhow, ensure};
use reqwest::{Client, Response, StatusCode, Url, header::CONTENT_TYPE, redirect::Policy};

/// Longest silence tolerated between streamed chunks; reasoning models can pause for a while.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
pub const EVENT_STREAM: &str = "text/event-stream";

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

/// Timeouts, conflicts, rate limits, overload and server errors may pass on a later
/// attempt.
pub fn is_retryable(status: StatusCode) -> bool {
    matches!(status.as_u16(), 408 | 409 | 429) || status.is_server_error()
}

/// A failure worth another attempt, carrying the provider's `retry-after` in seconds
/// when it gave one, capped so a bad header cannot stall the turn.
pub fn retry_failure(response: &Response, message: String) -> anyhow::Error {
    use crate::providers::{RetryAfter, Transient};
    const MAX_WAIT: Duration = Duration::from_secs(60);
    let wait = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds).min(MAX_WAIT));
    let error = anyhow::Error::new(Transient(message));
    match wait {
        Some(wait) => error.context(RetryAfter(wait)),
        None => error,
    }
}

/// What a refused request tells the user; a bad key says so, with where to fix it.
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

    #[test]
    fn only_passing_failures_are_transient() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(StatusCode::BAD_REQUEST));
    }
}
