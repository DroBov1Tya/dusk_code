//! Reading an HTTP answer's body with a hard cap: refused by its stated length when
//! that is already too long, and stopped while it arrives once it passes the cap, so a
//! server cannot fill memory with an endless answer.

use std::fmt;

use reqwest::Response;

/// Why a body was not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    /// It says, or turned out, to be over the cap.
    TooLarge,
    /// The connection broke off or the body could not be decoded.
    Broken,
}

impl BodyError {
    /// The error in words about `what`, such as "provider model catalogue":
    /// "{what} is too large" or "cannot read {what}".
    pub fn about(self, what: &str) -> anyhow::Error {
        match self {
            BodyError::TooLarge => anyhow::anyhow!("{what} is too large"),
            BodyError::Broken => anyhow::anyhow!("cannot read {what}"),
        }
    }
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BodyError::TooLarge => "the answer is too large",
            BodyError::Broken => "the answer broke off",
        })
    }
}

impl std::error::Error for BodyError {}

/// The body of `response`, at most `max` bytes.
pub async fn read_limited(response: Response, max: usize) -> Result<Vec<u8>, BodyError> {
    read_limited_reporting(response, max, |_| {}).await
}

/// As [`read_limited`], telling `progress` how many bytes have arrived after each chunk.
pub async fn read_limited_reporting(
    mut response: Response,
    max: usize,
    mut progress: impl FnMut(usize),
) -> Result<Vec<u8>, BodyError> {
    if response
        .content_length()
        .is_some_and(|length| length > max as u64)
    {
        return Err(BodyError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| BodyError::Broken)? {
        if chunk.len() > max - bytes.len() {
            return Err(BodyError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
        progress(bytes.len());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// A server on this machine that answers once with `head` and then `body`, written
    /// in pieces; the URL to ask it at.
    async fn serve(head: String, body: Vec<u8>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            if socket.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            for piece in body.chunks(1000) {
                if socket.write_all(piece).await.is_err() {
                    return;
                }
            }
            let _ = socket.shutdown().await;
        });
        format!("http://{address}/")
    }

    async fn get(url: &str) -> Response {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn a_body_within_the_cap_is_read_whole() {
        let body = vec![b'a'; 5000];
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        let url = serve(head, body.clone()).await;
        let mut seen = Vec::new();
        let read = read_limited_reporting(get(&url).await, 5000, |n| seen.push(n))
            .await
            .unwrap();
        assert_eq!(read, body);
        assert_eq!(seen.last(), Some(&5000));
    }

    #[tokio::test]
    async fn a_stated_length_over_the_cap_is_refused_unread() {
        let head = "HTTP/1.1 200 OK\r\nContent-Length: 1000000000\r\n\r\n".to_owned();
        let url = serve(head, Vec::new()).await;
        assert_eq!(
            read_limited(get(&url).await, 1024).await,
            Err(BodyError::TooLarge)
        );
    }

    #[tokio::test]
    async fn an_unstated_length_stops_at_the_cap() {
        // No length and no end until the connection closes: read until it passes the cap.
        let head = "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n".to_owned();
        let url = serve(head, vec![b'x'; 200_000]).await;
        assert_eq!(
            read_limited(get(&url).await, 4096).await,
            Err(BodyError::TooLarge)
        );
    }

    #[test]
    fn the_error_names_what_was_read() {
        assert_eq!(
            BodyError::TooLarge.about("the catalogue").to_string(),
            "the catalogue is too large"
        );
        assert_eq!(
            BodyError::Broken.about("the catalogue").to_string(),
            "cannot read the catalogue"
        );
    }
}
