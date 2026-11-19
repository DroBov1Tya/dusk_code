//! Text from a program the core does not trust: made plain and capped, links https only,
//! times as Unix seconds.

/// Untrusted text as plain text: control and direction-changing characters out, capped.
pub fn plain(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n') && !is_bidi(*c))
        .take(max)
        .collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
}

/// Untrusted text as one plain line, capped.
pub fn line(text: &str, max: usize) -> String {
    plain(text, max).replace('\n', " ")
}

/// Marks that reorder text on screen, so a name could show other than it reads.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Unix seconds of a time such as `2026-11-18T12:07:21Z`; `None` for anything else, and
/// for the zero time a forge gives a check not started.
pub fn time(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp())
        .filter(|seconds| *seconds > 0)
}

/// An https link of the safe characters, or nothing.
pub fn link(url: &str) -> String {
    let safe = url.starts_with("https://")
        && url.len() <= 500
        && url
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c));
    if safe { url.to_owned() } else { String::new() }
}
