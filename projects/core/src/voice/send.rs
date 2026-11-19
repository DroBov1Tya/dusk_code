//! Sending a request and reading its answer within bounds.

use anyhow::{Result, bail};
use std::time::Duration;

pub(super) const TIMEOUT: Duration = Duration::from_secs(120);

/// A transcript is text; far more than a recording's words means a wrong server.
pub(super) const MAX_REPLY_BYTES: usize = 1024 * 1024;

pub(super) const MAX_SOUND_BYTES: usize = 32 * 1024 * 1024;

/// How much of a server's complaint is shown.
pub(super) const MAX_COMPLAINT: usize = 200;

/// Sends a request and reads its answer, at most `limit` bytes.
pub(super) async fn send(
    request: reqwest::RequestBuilder,
    key: Option<&str>,
    limit: usize,
) -> Result<Vec<u8>> {
    let request = match key.filter(|key| !key.is_empty()) {
        Some(key) => request.bearer_auth(key),
        None => request,
    };
    let response = request.send().await.map_err(|error| {
        // The address may carry a secret in a query; the error is cut to its kind.
        anyhow::anyhow!("cannot reach the speech server: {}", error.without_url())
    })?;
    let status = response.status();
    let bytes = crate::net::read_limited(response, limit)
        .await
        .map_err(|error| error.about("the speech server's answer"))?;
    if !status.is_success() {
        let complaint: String = String::from_utf8_lossy(&bytes)
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_COMPLAINT)
            .collect();
        bail!("the speech server answered {status}: {complaint}");
    }
    Ok(bytes)
}
