use std::net::SocketAddr;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::*;

const TOKEN: &str = "capability";

fn auth(token: &str) -> String {
    format!(
        "Proxy-Authorization: Basic {}\r\n",
        STANDARD.encode(format!("dusk:{token}"))
    )
}

async fn ask(broker: &Broker, request: String) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", broker.port()))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).await.unwrap();
    answer
}

/// A server on this machine that answers with what it was sent.
async fn echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = vec![0u8; 8192];
            let read = stream.read(&mut buffer).await.unwrap();
            let seen = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let answer = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{seen}",
                seen.len()
            );
            let _ = stream.write_all(answer.as_bytes()).await;
        }
    });
    address
}

fn local(address: SocketAddr) -> Origin {
    Origin {
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: address.port(),
    }
}

fn policy(origins: Vec<Origin>, key_origins: Vec<Origin>) -> Policy {
    Policy::new(
        Secret(TOKEN.into()),
        origins,
        vec![Key {
            name: "token".into(),
            value: Secret("123:abc".into()),
            origins: key_origins,
        }],
    )
}

#[tokio::test]
async fn a_request_without_the_token_is_refused() {
    let broker = Broker::start(policy(Vec::new(), Vec::new())).unwrap();
    let request = "GET https://example.com/ HTTP/1.1\r\nHost: example.com\r\n\r\n";
    assert!(
        ask(&broker, request.into())
            .await
            .starts_with("HTTP/1.1 407")
    );
    let wrong = format!("GET https://example.com/ HTTP/1.1\r\n{}\r\n", auth("wrong"));
    assert!(ask(&broker, wrong).await.starts_with("HTTP/1.1 407"));
}

#[tokio::test]
async fn an_unlisted_origin_or_a_private_address_is_refused() {
    let server = echo_server().await;
    let listed = Origin::parse(&format!("https://localhost:{}", server.port())).unwrap();
    let broker = Broker::start(policy(vec![listed.clone()], vec![listed])).unwrap();
    let unlisted = format!("GET https://example.com/ HTTP/1.1\r\n{}\r\n", auth(TOKEN));
    let answer = ask(&broker, unlisted).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    // Listed, but it resolves to this machine.
    let private = format!(
        "GET https://localhost:{}/ HTTP/1.1\r\n{}\r\n",
        server.port(),
        auth(TOKEN)
    );
    let answer = ask(&broker, private).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    let connect = format!("CONNECT example.com:443 HTTP/1.1\r\n{}\r\n", auth(TOKEN));
    assert!(ask(&broker, connect).await.starts_with("HTTP/1.1 403"));
    let connect = format!(
        "CONNECT localhost:{} HTTP/1.1\r\n{}\r\n",
        server.port(),
        auth(TOKEN)
    );
    assert!(ask(&broker, connect).await.starts_with("HTTP/1.1 403"));
}

#[tokio::test]
async fn a_key_is_put_in_for_its_origin_only() {
    let server = echo_server().await;
    let other = Origin::parse("https://example.com").unwrap();
    let mut policy = policy(vec![local(server), other], vec![local(server)]);
    policy.public_only = false;
    let broker = Broker::start(policy).unwrap();
    let request = format!(
        "GET http://127.0.0.1:{}/bot{{{{key:token}}}}/getMe HTTP/1.1\r\n{}\
         X-Api-Key: {{{{key:token}}}}\r\n\r\n",
        server.port(),
        auth(TOKEN)
    );
    let answer = ask(&broker, request).await;
    assert!(answer.starts_with("HTTP/1.1 200"), "{answer}");
    // The server got the key; what came back has it hidden.
    assert!(answer.contains("/bot[hidden]/getMe"), "{answer}");
    assert!(
        answer.to_ascii_lowercase().contains("x-api-key: [hidden]"),
        "{answer}"
    );
    assert!(!answer.contains("123:abc"));
    assert!(!answer.to_ascii_lowercase().contains("proxy-authorization"));
    let elsewhere = format!(
        "GET https://example.com/{{{{key:token}}}} HTTP/1.1\r\n{}\r\n",
        auth(TOKEN)
    );
    let answer = ask(&broker, elsewhere).await;
    assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
    assert!(!answer.contains("123:abc"));
}
