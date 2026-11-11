//! Speech through an OpenAI-compatible server: a recording goes to `/audio/transcriptions`
//! and comes back as text, a text goes to `/audio/speech` and comes back as sound. The
//! server is whatever the user set up: a hosted service, or a model run on this machine.
//! The key travels as a bearer token and is never logged; a redirect is never followed, so
//! the key stays with the server it was given to.

use std::time::Duration;

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::voice::{MAX_RECORDING_BYTES, MAX_SPEECH_CHARS, VoiceEndpoint};
use serde::Deserialize;

use crate::providers::transport::http;

const TIMEOUT: Duration = Duration::from_secs(120);
/// A transcript is text; far more than a recording's words means a wrong server.
const MAX_REPLY_BYTES: usize = 1024 * 1024;
const MAX_SOUND_BYTES: usize = 32 * 1024 * 1024;
/// How much of a server's complaint is shown.
const MAX_COMPLAINT: usize = 200;

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

/// Sends a request and reads its answer, at most `limit` bytes.
async fn send(
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

/// A multipart body with text fields and one file.
fn multipart(boundary: &str, fields: &[(&str, &str)], file_name: &str, file: &[u8]) -> Vec<u8> {
    let mut body = Vec::with_capacity(file.len() + 512);
    for (name, value) in fields {
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            )
            .as_bytes(),
        );
    }
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\n\
             Content-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(file);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// A message as it is read aloud: code blocks are left out, links say their words, and the
/// marks of Markdown go, so the voice does not spell them. Cut at the limit.
pub fn speech_text(markdown: &str) -> String {
    let mut spoken = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let line = line.trim_start_matches(['#', '>', ' ']);
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        spoken.push_str(&plain_line(line));
        spoken.push('\n');
    }
    let spoken: String = spoken.split_whitespace().collect::<Vec<_>>().join(" ");
    spoken.chars().take(MAX_SPEECH_CHARS).collect()
}

/// A line without emphasis marks, inline code ticks and link targets.
fn plain_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' => {}
            // `[words](target)` reads as its words.
            ']' if chars.peek() == Some(&'(') => {
                for skipped in chars.by_ref() {
                    if skipped == ')' {
                        break;
                    }
                }
            }
            '[' => {}
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    /// A one-shot HTTP server on this machine: answers the first request with `reply` and
    /// gives back the request it got.
    async fn serve(
        status: &str,
        content_type: &str,
        reply: Vec<u8>,
    ) -> (String, tokio::task::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            reply.len()
        );
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request).to_string();
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
                if read == 0 {
                    break;
                }
            }
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(&reply).await.unwrap();
            String::from_utf8_lossy(&request).to_string()
        });
        (base, task)
    }

    fn endpoint(base_url: String) -> VoiceEndpoint {
        VoiceEndpoint {
            base_url,
            model: "whisper-1".into(),
            voice: Some("alloy".into()),
        }
    }

    #[tokio::test]
    async fn a_recording_goes_up_with_the_key_and_the_text_comes_back() {
        let (base, request) = serve(
            "200 OK",
            "application/json",
            br#"{"text":" hello there "}"#.to_vec(),
        )
        .await;
        let text = transcribe(&endpoint(base), Some("sk-secret"), b"RIFFxxxxWAVE")
            .await
            .unwrap();

        assert_eq!(text, "hello there");
        let request = request.await.unwrap();
        assert!(request.starts_with("POST /v1/audio/transcriptions"));
        assert!(
            request
                .to_lowercase()
                .contains("authorization: bearer sk-secret")
        );
        assert!(request.contains("name=\"model\"\r\n\r\nwhisper-1"));
        assert!(request.contains("filename=\"recording.wav\""));
    }

    #[tokio::test]
    async fn a_refusal_is_reported_without_the_key() {
        let (base, _) = serve(
            "401 Unauthorized",
            "application/json",
            br#"{"error":"bad key"}"#.to_vec(),
        )
        .await;
        let error = transcribe(&endpoint(base), Some("sk-secret"), b"RIFF")
            .await
            .unwrap_err();

        let shown = format!("{error:#}");
        assert!(shown.contains("401"), "{shown}");
        assert!(!shown.contains("sk-secret"), "{shown}");
    }

    #[tokio::test]
    async fn a_text_goes_up_as_json_and_the_sound_comes_back() {
        let (base, request) = serve("200 OK", "audio/mpeg", b"ID3sound".to_vec()).await;
        let sound = speak(&endpoint(base), None, "Hello **world**")
            .await
            .unwrap();

        assert_eq!(sound, b"ID3sound");
        let request = request.await.unwrap();
        assert!(request.starts_with("POST /v1/audio/speech"));
        assert!(request.contains("\"input\":\"Hello world\""), "{request}");
        assert!(request.contains("\"voice\":\"alloy\""));
        assert!(!request.to_lowercase().contains("authorization"));
    }

    #[tokio::test]
    async fn an_endpoint_over_plain_http_to_the_internet_is_refused() {
        let remote = endpoint("http://example.com/v1".into());
        assert!(transcribe(&remote, None, b"RIFF").await.is_err());
        assert!(check_endpoint(&remote).is_err());
        assert!(check_endpoint(&endpoint("https://api.example.com/v1".into())).is_ok());
        assert!(check_endpoint(&endpoint("http://user:pw@127.0.0.1/v1".into())).is_err());
        let mut injected = endpoint("https://api.example.com/v1".into());
        injected.model = "x\r\n--boundary".into();
        assert!(check_endpoint(&injected).is_err());
    }

    #[tokio::test]
    async fn an_empty_or_huge_recording_is_refused_before_it_is_sent() {
        let local = endpoint("http://127.0.0.1:9/v1".into());
        assert!(transcribe(&local, None, b"").await.is_err());
        let huge = vec![0u8; MAX_RECORDING_BYTES + 1];
        assert!(transcribe(&local, None, &huge).await.is_err());
    }

    #[test]
    fn a_message_is_read_without_its_marks_and_code() {
        let text = "# Title\nSome **bold** and `code` and [a link](https://x.y/z).\n```rust\nlet a = 1;\n```\n- item one";
        assert_eq!(
            speech_text(text),
            "Title Some bold and code and a link. item one"
        );
        assert_eq!(speech_text("```\nonly code\n```"), "");
        assert_eq!(
            speech_text(&"a".repeat(MAX_SPEECH_CHARS + 50)).len(),
            MAX_SPEECH_CHARS
        );
    }
}
