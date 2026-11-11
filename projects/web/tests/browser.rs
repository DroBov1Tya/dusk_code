//! The server against a real core: what a browser may and may not do.

use dusk_core::app::{Paths, start_hub};
use dusk_web::{Config, WebServer, start};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn server() -> (WebServer, std::path::PathBuf) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("dusk-web-{}-{n}", std::process::id()));
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    (start(hub, Config::default()).await.unwrap(), root)
}

/// One raw HTTP/1.1 exchange; the whole answer as text.
async fn http(server: &WebServer, request: &str) -> String {
    let mut stream = TcpStream::connect(server.addr()).await.unwrap();
    let request = request.replace("{addr}", &server.addr().to_string());
    stream.write_all(request.as_bytes()).await.unwrap();
    // An upgrade request keeps its connection; the head of the answer is what matters.
    let mut answer = Vec::new();
    let read = async {
        let mut chunk = [0u8; 4096];
        loop {
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => answer.extend_from_slice(&chunk[..n]),
            }
            if answer.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(3), read).await;
    String::from_utf8_lossy(&answer).into_owned()
}

fn post_login(token: &str, origin: &str, host: &str) -> String {
    let body = format!(r#"{{"token":"{token}"}}"#);
    format!(
        "POST /api/login HTTP/1.1\r\nHost: {host}\r\nOrigin: {origin}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

#[tokio::test]
async fn the_page_has_the_security_headers_and_the_stream_needs_a_session() {
    let (server, root) = server().await;

    let page = http(
        &server,
        "GET / HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n",
    )
    .await;
    let page = page.to_ascii_lowercase();
    assert!(page.starts_with("http/1.1 200"), "{page}");
    for header in [
        "content-security-policy:",
        "frame-ancestors 'none'",
        "x-content-type-options: nosniff",
        "referrer-policy: no-referrer",
    ] {
        assert!(page.contains(header), "missing {header}");
    }

    let rebound = http(
        &server,
        "GET / HTTP/1.1\r\nHost: evil.example\r\nConnection: close\r\n\r\n",
    )
    .await;
    assert!(rebound.starts_with("HTTP/1.1 403"), "{rebound}");

    let upgrade = "GET /api/ws HTTP/1.1\r\nHost: {addr}\r\nOrigin: http://{addr}\r\n\
        Connection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\n\
        Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n";
    let unsigned = http(&server, upgrade).await;
    assert!(unsigned.starts_with("HTTP/1.1 401"), "{unsigned}");

    server.stop().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn signing_in_checks_the_token_the_origin_and_the_host() {
    let (server, root) = server().await;
    let addr = server.addr().to_string();
    let own = format!("http://{addr}");

    let foreign = http(
        &server,
        &post_login(&server.sign_in_url(), "http://evil.example", &addr),
    )
    .await;
    assert!(foreign.starts_with("HTTP/1.1 403"), "{foreign}");

    let wrong = http(&server, &post_login("nope", &own, &addr)).await;
    assert!(wrong.starts_with("HTTP/1.1 401"), "{wrong}");

    let token = server.sign_in_url().rsplit('#').next().unwrap().to_owned();
    let right = http(&server, &post_login(&token, &own, &addr))
        .await
        .to_ascii_lowercase();
    assert!(right.starts_with("http/1.1 204"), "{right}");
    assert!(right.contains("set-cookie: __host-dusk="), "{right}");
    assert!(
        right.contains("httponly") && right.contains("samesite=strict"),
        "{right}"
    );

    server.stop().await;
    let _ = std::fs::remove_dir_all(root);
}
