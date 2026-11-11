//! What every request must satisfy before it is served, and the headers every answer has.
//! The server binds a known address, so a request for another host is DNS rebinding and
//! one from another origin is a page of someone else talking to it.

use axum::http::{HeaderMap, HeaderName, HeaderValue, header};
use std::net::{IpAddr, SocketAddr};

/// The host names a request may be addressed to, as the browser writes them in `Host`.
pub struct Hosts(Vec<String>);

impl Hosts {
    /// The names that reach `bound`, plus those the user allowed for a proxy or a name.
    pub fn for_bind(bound: SocketAddr, extra: &[String]) -> Self {
        let port = bound.port();
        let mut hosts = Vec::new();
        let ip = bound.ip();
        if ip.is_loopback() || ip.is_unspecified() {
            hosts.extend([
                format!("localhost:{port}"),
                format!("127.0.0.1:{port}"),
                format!("[::1]:{port}"),
            ]);
        }
        if !ip.is_unspecified() {
            hosts.push(SocketAddr::new(ip, port).to_string());
        }
        hosts.extend(extra.iter().map(|host| host.to_ascii_lowercase()));
        hosts.sort();
        hosts.dedup();
        Self(hosts)
    }

    /// The `Host` header must name this server.
    pub fn host_ok(&self, headers: &HeaderMap) -> bool {
        header_text(headers, &header::HOST).is_some_and(|host| self.allows(host))
    }

    /// `Origin` must be this server's own page. Browsers always send it on a WebSocket
    /// upgrade and on a POST, so a missing one is refused where `required`.
    pub fn origin_ok(&self, headers: &HeaderMap, required: bool) -> bool {
        match header_text(headers, &header::ORIGIN) {
            None => !required,
            Some(origin) => origin
                .strip_prefix("http://")
                .is_some_and(|host| self.allows(host)),
        }
    }

    fn allows(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.0.contains(&host)
    }

    pub fn first(&self) -> &str {
        self.0.first().map_or("localhost", String::as_str)
    }
}

fn header_text<'a>(headers: &'a HeaderMap, name: &HeaderName) -> Option<&'a str> {
    headers.get(name)?.to_str().ok()
}

/// Whether `ip` is this machine only.
pub fn is_local(ip: IpAddr) -> bool {
    ip.is_loopback()
}

/// The headers of every answer. `script_hashes` are the `sha256-` sources of the loader
/// the bundle inlines; no other script may run, and the wasm may only be compiled.
pub fn secure(headers: &mut HeaderMap, script_hashes: &str, host: &str) {
    let csp = format!(
        "default-src 'none'; script-src 'self' 'wasm-unsafe-eval' {script_hashes}; \
         style-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; \
         connect-src 'self' ws://{host}; base-uri 'none'; form-action 'none'; \
         frame-ancestors 'none'; object-src 'none'"
    );
    let mut put = |name: HeaderName, value: &str| {
        if let Ok(value) = HeaderValue::from_str(value) {
            headers.insert(name, value);
        }
    };
    put(header::CONTENT_SECURITY_POLICY, &csp);
    put(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    put(header::REFERRER_POLICY, "no-referrer");
    put(header::X_FRAME_OPTIONS, "DENY");
    put(header::CACHE_CONTROL, "no-store");
    put(
        HeaderName::from_static("permissions-policy"),
        "camera=(), geolocation=(), microphone=(self)",
    );
    put(
        HeaderName::from_static("cross-origin-opener-policy"),
        "same-origin",
    );
    put(
        HeaderName::from_static("cross-origin-resource-policy"),
        "same-origin",
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn local() -> Hosts {
        Hosts::for_bind("127.0.0.1:7777".parse().unwrap(), &[])
    }

    #[test]
    fn only_the_servers_own_names_pass_the_host_check() {
        let hosts = local();
        assert!(hosts.host_ok(&headers(&[("host", "127.0.0.1:7777")])));
        assert!(hosts.host_ok(&headers(&[("host", "LOCALHOST:7777")])));
        assert!(!hosts.host_ok(&headers(&[("host", "evil.example:7777")])));
        assert!(!hosts.host_ok(&headers(&[("host", "localhost:80")])));
        assert!(!hosts.host_ok(&headers(&[])));
    }

    #[test]
    fn a_foreign_origin_is_refused_and_a_missing_one_only_where_required() {
        let hosts = local();
        assert!(hosts.origin_ok(&headers(&[("origin", "http://localhost:7777")]), true));
        assert!(!hosts.origin_ok(&headers(&[("origin", "http://evil.example")]), true));
        assert!(!hosts.origin_ok(&headers(&[("origin", "https://localhost:7777")]), true));
        assert!(!hosts.origin_ok(&headers(&[("origin", "null")]), false));
        assert!(!hosts.origin_ok(&headers(&[]), true));
        assert!(hosts.origin_ok(&headers(&[]), false));
    }

    #[test]
    fn an_open_bind_allows_only_the_names_given() {
        let hosts = Hosts::for_bind("0.0.0.0:80".parse().unwrap(), &["Box.lan:80".into()]);
        assert!(hosts.host_ok(&headers(&[("host", "box.lan:80")])));
        assert!(!hosts.host_ok(&headers(&[("host", "0.0.0.0:80")])));
    }
}
