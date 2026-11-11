//! Sending a request and reading its answer within bounds.

use anyhow::{Context, Result, bail, ensure};
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
    let mut response = request.send().await.map_err(|error| {
        // The address may carry a secret in a query; the error is cut to its kind.
        anyhow::anyhow!("cannot reach the speech server: {}", error.without_url())
    })?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.context("the answer broke off")? {
        bytes.extend_from_slice(&chunk);
        ensure!(
            bytes.len() <= limit,
            "the speech server's answer is too large"
        );
    }
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
