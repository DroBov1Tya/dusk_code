//! The name a provider connected from the welcome gets: the host of its address, as the
//! model menu shows it. Anything odd gives no name, and the core picks `provider-N`.

/// The core's own limit on a provider's name.
const MAX: usize = 40;

/// `api.example.com` from `https://user@API.example.com:8443/v1?x`.
pub fn from_address(address: &str) -> Option<String> {
    let address = address.trim();
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next()?;
    let host_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = match host_port.strip_prefix('[') {
        Some(bracketed) => bracketed.split_once(']')?.0,
        None => host_port.split(':').next()?,
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let plain = host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':'));
    (!host.is_empty() && host.len() <= MAX && plain).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::from_address;

    #[test]
    fn takes_the_host_and_refuses_odd_input() {
        let named = |address| from_address(address);
        assert_eq!(
            named(" https://user:pw@API.Example.com.:8443/v1?x#y "),
            Some("api.example.com".into())
        );
        assert_eq!(named("http://localhost:11434/v1"), Some("localhost".into()));
        assert_eq!(named("http://[::1]:8080/v1"), Some("::1".into()));
        assert_eq!(named("192.168.1.5:1234"), Some("192.168.1.5".into()));
        for odd in [
            "",
            "   ",
            "https://",
            "http://[::1",
            "http://a b/v1",
            "http://e\u{7}x/v1",
        ] {
            assert_eq!(named(odd), None, "{odd:?}");
        }
        assert_eq!(named(&format!("https://{}.com", "a".repeat(40))), None);
    }
}
