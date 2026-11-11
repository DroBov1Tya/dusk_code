//! A text in, its sound out.

use super::markdown::speech_text;
use super::send::{MAX_SOUND_BYTES, TIMEOUT, send};
use crate::providers::transport::http;
use anyhow::{Result, ensure};
use dusk_protocol::config::VoiceEndpoint;

/// The sound of a text, as the server sent it; the caller checks what it is.
pub async fn speak(endpoint: &VoiceEndpoint, key: Option<&str>, text: &str) -> Result<Vec<u8>> {
    let text = speech_text(text);
    ensure!(!text.is_empty(), "there is nothing to read aloud");
    let url = http::endpoint(
        &http::base_url(endpoint.base_url.trim(), false)?,
        &["audio", "speech"],
    )?;
    let mut body = serde_json::json!({
        "model": endpoint.model.trim(),
        "input": text,
        "response_format": "mp3",
    });
    if let Some(voice) = endpoint
        .voice
        .as_deref()
        .filter(|voice| !voice.trim().is_empty())
    {
        body["voice"] = voice.trim().into();
    }
    let request = http::client()?.post(url).timeout(TIMEOUT).json(&body);
    send(request, key, MAX_SOUND_BYTES).await
}
