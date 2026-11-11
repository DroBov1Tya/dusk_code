use super::markdown::speech_text;
use super::{check_endpoint, speak, transcribe};
use dusk_protocol::config::{MAX_RECORDING_BYTES, MAX_SPEECH_CHARS, VoiceEndpoint};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

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
