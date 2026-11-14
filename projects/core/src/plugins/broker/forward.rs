//! One request in the absolute form, sent on to an allowed origin with the plugin's keys
//! put in, and its whole answer written back.

use std::time::Duration;

use reqwest::{
    Method, Url,
    header::{HeaderMap, HeaderName, HeaderValue},
    redirect,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

use super::{Head, Refused, policy::Origin, policy::Policy, refuse};

/// The most a request or an answer may carry.
pub(super) const MAX_BODY: usize = 16 * 1024 * 1024;
const TIME: Duration = Duration::from_secs(60);
/// Headers about this one hop, which are not passed on either way.
const HOP_BY_HOP: [&str; 11] = [
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "host",
    "content-length",
];
/// What stands in an answer where a key the request carried was.
const HIDDEN: &[u8] = b"[hidden]";

pub(super) async fn send(
    stream: &mut TcpStream,
    head: &Head,
    rest: Vec<u8>,
    policy: &Policy,
) -> Result<(), Refused> {
    let (url, origin) = match target(&head.target, policy) {
        Ok(found) => found,
        Err(reason) => return Err(refuse(stream, 403, reason).await),
    };
    if !policy.allows(&origin) {
        return Err(refuse(stream, 403, "this origin is not allowed".into()).await);
    }
    let (url, mut used) = match policy.put_keys(&url, &origin) {
        Ok(put) => put,
        Err(error) => return Err(refuse(stream, 403, format!("{error:#}")).await),
    };
    // Put in, the URL must still name the same origin.
    let url = match Url::parse(&url) {
        Ok(url) if Origin::of(&url).is_ok_and(|after| after == origin) => url,
        _ => return Err(refuse(stream, 400, "the target is not a URL".into()).await),
    };
    let mut headers = HeaderMap::new();
    for (name, value) in &head.headers {
        let lower = name.to_ascii_lowercase();
        if HOP_BY_HOP.contains(&lower.as_str()) {
            continue;
        }
        let Ok(text) = std::str::from_utf8(value) else {
            return Err(refuse(stream, 400, format!("header {name} is not text")).await);
        };
        let (text, keys) = match policy.put_keys(text, &origin) {
            Ok(put) => put,
            Err(error) => return Err(refuse(stream, 403, format!("{error:#}")).await),
        };
        used.extend(keys);
        match (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&text),
        ) {
            (Ok(name), Ok(value)) => {
                headers.append(name, value);
            }
            _ => return Err(refuse(stream, 400, format!("header {name} is not valid")).await),
        }
    }
    if head.header("transfer-encoding").is_some() {
        let reason = "send a body with Content-Length".to_owned();
        return Err(refuse(stream, 411, reason).await);
    }
    let length = match head.header("content-length").map(parse_length) {
        None => 0,
        Some(Some(length)) if length <= MAX_BODY => length,
        Some(_) => return Err(refuse(stream, 413, "the body is too large".into()).await),
    };
    let body = match read_body(stream, rest, length).await {
        Ok(body) => body,
        Err(reason) => return Err(refuse(stream, 400, reason).await),
    };
    let addresses = match policy.resolve(&origin.host, origin.port).await {
        Ok(addresses) => addresses,
        Err(error) => return Err(refuse(stream, 403, format!("{error:#}")).await),
    };
    let Ok(method) = Method::from_bytes(head.method.as_bytes()) else {
        return Err(refuse(stream, 400, "not a method".into()).await);
    };
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(redirect::Policy::none())
        .https_only(policy.public_only)
        .timeout(TIME)
        .connect_timeout(Duration::from_secs(10))
        // The addresses checked are the ones connected to.
        .resolve_to_addrs(&origin.host, &addresses)
        .build();
    let Ok(client) = client else {
        return Err(refuse(stream, 502, "cannot make a client".into()).await);
    };
    let sent = client
        .request(method, url)
        .headers(headers)
        .body(body)
        .send()
        .await;
    // The error of reqwest names the URL, which may hold a key: it is not passed on.
    let mut response = match sent {
        Ok(response) => response,
        Err(error) if error.is_timeout() => {
            return Err(refuse(stream, 504, "the origin did not answer in time".into()).await);
        }
        Err(_) => return Err(refuse(stream, 502, "the origin could not be reached".into()).await),
    };
    let status = response.status();
    let mut answer = Vec::new();
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) if answer.len() + chunk.len() <= MAX_BODY => {
                answer.extend_from_slice(&chunk)
            }
            Ok(Some(_)) => {
                return Err(refuse(stream, 502, "the answer is too large".into()).await);
            }
            Ok(None) => break,
            Err(_) => return Err(refuse(stream, 502, "the answer broke off".into()).await),
        }
    }
    let secrets: Vec<&[u8]> = used.iter().map(|key| key.0.as_bytes()).collect();
    let mut out = format!(
        "HTTP/1.1 {} {}\r\n",
        status.as_u16(),
        status.canonical_reason().unwrap_or("")
    )
    .into_bytes();
    for (name, value) in response.headers() {
        if HOP_BY_HOP.contains(&name.as_str()) {
            continue;
        }
        out.extend_from_slice(name.as_str().as_bytes());
        out.extend_from_slice(b": ");
        out.extend_from_slice(&hide(value.as_bytes(), &secrets));
        out.extend_from_slice(b"\r\n");
    }
    let answer = hide(&answer, &secrets);
    out.extend_from_slice(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            answer.len()
        )
        .as_bytes(),
    );
    out.extend_from_slice(&answer);
    let _ = stream.write_all(&out).await;
    Ok(())
}

/// The absolute target and its origin, the keys not yet in.
fn target(text: &str, policy: &Policy) -> Result<(String, Origin), String> {
    let url = Url::parse(text).map_err(|_| "the target must be an absolute URL".to_owned())?;
    let origin = Origin::of(&url).map_err(|error| format!("{error:#}"))?;
    if policy.public_only && origin.scheme != "https" {
        return Err("only https targets are let through".into());
    }
    Ok((text.to_owned(), origin))
}

fn parse_length(value: &[u8]) -> Option<usize> {
    std::str::from_utf8(value).ok()?.trim().parse().ok()
}

async fn read_body(
    stream: &mut TcpStream,
    mut body: Vec<u8>,
    length: usize,
) -> Result<Vec<u8>, String> {
    if body.len() > length {
        return Err("more bytes than Content-Length".into());
    }
    let mut chunk = [0u8; 16 * 1024];
    let read = async {
        while body.len() < length {
            let wanted = (length - body.len()).min(chunk.len());
            let read = stream
                .read(&mut chunk[..wanted])
                .await
                .map_err(|_| "the body broke off".to_owned())?;
            if read == 0 {
                return Err("the body broke off".to_owned());
            }
            body.extend_from_slice(&chunk[..read]);
        }
        Ok(())
    };
    tokio::time::timeout(TIME, read)
        .await
        .map_err(|_| "the body did not come in time".to_owned())??;
    Ok(body)
}

/// `bytes` with every one of `secrets` replaced, so an origin that echoes a key does
/// not hand it to the plugin.
fn hide(bytes: &[u8], secrets: &[&[u8]]) -> Vec<u8> {
    let mut out = bytes.to_vec();
    for secret in secrets.iter().filter(|secret| !secret.is_empty()) {
        let mut replaced = Vec::with_capacity(out.len());
        let mut index = 0;
        while index < out.len() {
            if out[index..].starts_with(secret) {
                replaced.extend_from_slice(HIDDEN);
                index += secret.len();
            } else {
                replaced.push(out[index]);
                index += 1;
            }
        }
        out = replaced;
    }
    out
}
