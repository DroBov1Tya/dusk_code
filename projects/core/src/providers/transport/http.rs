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

/// The configured base URL: HTTPS, or HTTP to this machine, with no credentials, query
/// or fragment in it.
pub fn base_url(text: &str) -> Result<Url> {
    let url = Url::parse(text).map_err(|_| anyhow!("invalid provider base URL"))?;
    ensure!(
        url.scheme() == "https" && url.host_str().is_some() || is_loopback(&url),
        "provider base URL must use HTTPS, or HTTP to 127.0.0.1 or [::1]"
    );
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
    fn plain_http_reaches_only_this_machine() {
        let url = |text: &str| Url::parse(text).unwrap();
        assert!(is_loopback(&url("http://127.0.0.1:8080/v1")));
        assert!(is_loopback(&url("http://[::1]:8080/v1")));
        for remote in [
            "http://localhost/v1",
            "http://10.0.0.1/v1",
            "http://example.com/v1",
        ] {
            assert!(!is_loopback(&url(remote)), "{remote}");
            assert!(base_url(remote).is_err(), "{remote}");
        }
        assert!(base_url("https://user:pass@api.example.com/v1").is_err());
        assert!(base_url("https://api.example.com/v1").is_ok());
    }

    #[test]
    fn only_passing_failures_are_transient() {
        assert!(is_retryable(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable(StatusCode::BAD_GATEWAY));
        assert!(!is_retryable(StatusCode::UNAUTHORIZED));
        assert!(!is_retryable(StatusCode::BAD_REQUEST));
    }
}
