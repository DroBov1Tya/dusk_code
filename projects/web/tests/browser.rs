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

// The stream itself.

use dusk_protocol::client::{Event, Request, RequestFrame, Response, ServerFrame, ThreadRequest};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async, tungstenite::Message, tungstenite::client::IntoClientRequest,
};

type Stream = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

/// A session cookie as a browser would send it back.
async fn sign_in(server: &WebServer) -> String {
    let addr = server.addr().to_string();
    let token = server.sign_in_url().rsplit('#').next().unwrap().to_owned();
    let answer = http(
        server,
        &post_login(&token, &format!("http://{addr}"), &addr),
    )
    .await;
    let line = answer
        .lines()
        .find(|line| line.to_ascii_lowercase().starts_with("set-cookie:"))
        .expect("a cookie");
    line.split_once(':')
        .unwrap()
        .1
        .split(';')
        .next()
        .unwrap()
        .trim()
        .to_owned()
}

async fn open(
    server: &WebServer,
    cookie: &str,
    origin: &str,
) -> Result<Stream, tokio_tungstenite::tungstenite::Error> {
    let mut request = format!("ws://{}/api/ws", server.addr())
        .into_client_request()
        .unwrap();
    let headers = request.headers_mut();
    headers.insert("cookie", cookie.parse().unwrap());
    headers.insert("origin", origin.parse().unwrap());
    connect_async(request).await.map(|(stream, _)| stream)
}

async fn frame(stream: &mut Stream) -> Option<ServerFrame> {
    loop {
        let next = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await;
        match next.ok()?? {
            Ok(Message::Text(text)) => return serde_json::from_str(&text).ok(),
            Ok(Message::Close(_)) | Err(_) => return None,
            Ok(_) => {}
        }
    }
}

async fn call(stream: &mut Stream, id: u64, request: Request) -> Response {
    let text = serde_json::to_string(&RequestFrame { id, request }).unwrap();
    stream.send(Message::Text(text.into())).await.unwrap();
    loop {
        match frame(stream).await.expect("the stream stays open") {
            ServerFrame::Response { id: got, response } if got == id => return response,
            _ => {}
        }
    }
}

#[tokio::test]
async fn the_stream_answers_requests_and_refuses_what_a_browser_may_not_do() {
    let (server, root) = server().await;
    let cookie = sign_in(&server).await;
    let origin = format!("http://{}", server.addr());

    let mut stream = open(&server, &cookie, &origin)
        .await
        .expect("a signed-in browser connects");
    assert!(matches!(
        frame(&mut stream).await,
        Some(ServerFrame::Status(_))
    ));
    let listed = call(&mut stream, 1, Request::Thread(ThreadRequest::List)).await;
    assert!(matches!(listed, Response::Threads(_)), "{listed:?}");
    let stopped = call(&mut stream, 2, Request::Shutdown).await;
    assert!(matches!(stopped, Response::Error(_)), "{stopped:?}");
    // The core is still there after that.
    let again = call(&mut stream, 3, Request::Thread(ThreadRequest::List)).await;
    assert!(matches!(again, Response::Threads(_)));

    server.stop().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn the_stream_refuses_a_foreign_origin_a_missing_session_and_bad_frames() {
    let (server, root) = server().await;
    let cookie = sign_in(&server).await;
    let origin = format!("http://{}", server.addr());

    assert!(open(&server, &cookie, "http://evil.example").await.is_err());
    assert!(open(&server, "dusk=nope", &origin).await.is_err());

    let mut stream = open(&server, &cookie, &origin).await.unwrap();
    stream.send(Message::Text("not json".into())).await.unwrap();
    // Status first, then the stream is closed on the bad frame.
    while frame(&mut stream).await.is_some() {}

    let mut stream = open(&server, &cookie, &origin).await.unwrap();
    let huge = " ".repeat(33 * 1024 * 1024);
    let _ = stream.send(Message::Text(huge.into())).await;
    while frame(&mut stream).await.is_some() {}

    server.stop().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn two_browsers_on_one_core_see_the_same_thread_events() {
    let (server, root) = server().await;
    let cookie = sign_in(&server).await;
    let origin = format!("http://{}", server.addr());
    let mut first = open(&server, &cookie, &origin).await.unwrap();
    let mut second = open(&server, &cookie, &origin).await.unwrap();
    let cwd = root.join("work");
    std::fs::create_dir_all(&cwd).unwrap();

    let Response::Opened { thread_id } = call(
        &mut first,
        1,
        Request::Thread(ThreadRequest::New { cwd: cwd.clone() }),
    )
    .await
    else {
        panic!("the thread did not open");
    };
    let joined = call(
        &mut second,
        1,
        Request::Thread(ThreadRequest::Resume {
            thread_id: thread_id.clone(),
            cwd,
        }),
    )
    .await;
    assert!(matches!(joined, Response::Opened { .. }), "{joined:?}");
    let sent = call(
        &mut first,
        2,
        Request::Thread(ThreadRequest::Send {
            thread_id: thread_id.clone(),
            text: "hello".into(),
            now: false,
        }),
    )
    .await;
    assert!(matches!(sent, Response::Done), "{sent:?}");

    for stream in [&mut first, &mut second] {
        let mut seen = false;
        while let Some(next) = frame(stream).await {
            if let ServerFrame::Event(Event::Thread { thread_id: id, .. }) = next
                && id == thread_id
            {
                seen = true;
                break;
            }
        }
        assert!(seen, "a browser missed the thread's events");
    }

    server.stop().await;
    let _ = std::fs::remove_dir_all(root);
}
