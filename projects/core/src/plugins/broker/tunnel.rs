//! `CONNECT host:port`: a tunnel to an allowed https origin, for a library that speaks
//! TLS itself. The broker sees only bytes then, so no key is put in; the origin and the
//! address are checked as for any request.

use std::time::Duration;

use tokio::{io::AsyncWriteExt, net::TcpStream};

use super::{Head, Refused, connect, policy::Origin, policy::Policy, refuse};

/// Long polls and kept-alive connections live in a tunnel; none lives longer than this.
const MAX_TIME: Duration = Duration::from_secs(60 * 60);

pub(super) async fn open(
    mut stream: TcpStream,
    head: &Head,
    rest: Vec<u8>,
    policy: &Policy,
) -> Result<(), Refused> {
    let Some(origin) = authority(&head.target) else {
        return Err(refuse(&mut stream, 400, "CONNECT names host:port".into()).await);
    };
    if !policy.allows(&origin) {
        return Err(refuse(&mut stream, 403, "this origin is not allowed".into()).await);
    }
    let addresses = match policy.resolve(&origin.host, origin.port).await {
        Ok(addresses) => addresses,
        Err(error) => return Err(refuse(&mut stream, 403, format!("{error:#}")).await),
    };
    let mut upstream = match connect(&addresses).await {
        Ok(upstream) => upstream,
        Err(_) => return Err(refuse(&mut stream, 502, "cannot reach the origin".into()).await),
    };
    let ready = b"HTTP/1.1 200 Connection Established\r\n\r\n";
    if stream.write_all(ready).await.is_err() || upstream.write_all(&rest).await.is_err() {
        return Ok(());
    }
    let _ = tokio::time::timeout(
        MAX_TIME,
        tokio::io::copy_bidirectional(&mut stream, &mut upstream),
    )
    .await;
    Ok(())
}

/// `host:port` as an https origin; a bracketed IPv6 host keeps its brackets out.
fn authority(target: &str) -> Option<Origin> {
    let (host, port) = target.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() || host.contains(['/', '@', '[', ']']) {
        return None;
    }
    Some(Origin {
        scheme: "https".into(),
        host: host.to_ascii_lowercase(),
        port: port.parse().ok()?,
    })
}
