//! Checking the address and names of a speech server.

use crate::providers::transport::http;
use anyhow::{Result, ensure};
use dusk_protocol::config::VoiceEndpoint;

/// The server of an endpoint, checked as a provider's is: HTTPS, or HTTP to this machine or a
/// private network, with no credentials, query or fragment in the address.
pub fn check_endpoint(endpoint: &VoiceEndpoint) -> Result<()> {
    http::base_url(endpoint.base_url.trim(), false)?;
    // The model goes into a multipart body as it is, so no line break may be in it.
    let plain = |text: &str| text.len() <= 200 && !text.chars().any(char::is_control);
    ensure!(
        !endpoint.model.trim().is_empty() && plain(&endpoint.model),
        "a model name is up to 200 characters, without control characters"
    );
    if let Some(voice) = &endpoint.voice {
        ensure!(
            plain(voice),
            "a voice name is up to 200 characters, without control characters"
        );
    }
    Ok(())
}
