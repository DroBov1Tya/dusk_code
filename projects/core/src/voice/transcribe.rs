//! A recording in, its text out.

use super::multipart::multipart;
use super::send::{MAX_REPLY_BYTES, TIMEOUT, send};
use crate::providers::transport::http;
use anyhow::{Context, Result, ensure};
use dusk_protocol::config::{MAX_RECORDING_BYTES, VoiceEndpoint};
use serde::Deserialize;

#[derive(Deserialize)]
struct Transcript {
    text: String,
}

/// The text of a WAV recording.
pub async fn transcribe(
    endpoint: &VoiceEndpoint,
    key: Option<&str>,
    audio: &[u8],
) -> Result<String> {
    ensure!(!audio.is_empty(), "the recording is empty");
    ensure!(
        audio.len() <= MAX_RECORDING_BYTES,
        "the recording is over {} MB",
        MAX_RECORDING_BYTES / 1024 / 1024
    );
    let url = http::endpoint(
        &http::base_url(endpoint.base_url.trim(), false)?,
        &["audio", "transcriptions"],
    )?;
    let boundary = format!("duskcode-{}", uuid::Uuid::new_v4().simple());
    let body = multipart(
        &boundary,
        &[
            ("model", endpoint.model.trim()),
            ("response_format", "json"),
        ],
        "recording.wav",
        audio,
    );
    let request = http::client()?
        .post(url)
        .timeout(TIMEOUT)
        .header(
            "Content-Type",
            format!("multipart/form-data; boundary={boundary}"),
        )
        .body(body);
    let bytes = send(request, key, MAX_REPLY_BYTES).await?;
    let transcript: Transcript =
        serde_json::from_slice(&bytes).context("the server did not answer with a transcript")?;
    Ok(transcript.text.trim().to_owned())
}
