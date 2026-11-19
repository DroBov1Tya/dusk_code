//! Text from a program the core does not trust: made plain and capped, links https only,
//! times as Unix seconds.

/// Untrusted text as plain text: control characters, line and paragraph separators and
/// invisible format characters (zero width, bidi) out, capped.
pub fn plain(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n') && !is_invisible(*c))
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

/// Characters that draw nothing or reorder text on screen (Unicode Zl, Zp and Cf), so a
/// name could show other than it reads or break a one-line log.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00ad}' | '\u{0600}'..='\u{0605}' | '\u{061c}' | '\u{06dd}' | '\u{070f}' | '\u{0890}'..='\u{0891}' | '\u{08e2}'
        | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{110bd}'
        | '\u{110cd}' | '\u{1bca0}'..='\u{1bca3}' | '\u{13430}'..='\u{1343f}' | '\u{1d173}'..='\u{1d17a}' | '\u{e0001}'
        | '\u{e0020}'..='\u{e007f}')
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invisible_and_separator_characters_are_dropped() {
        let text = "a\u{2028}b\u{2029}c\u{200b}d\u{feff}e\u{202e}f\u{2066}g\u{00ad}h";
        assert_eq!(plain(text, 100), "abcdefgh");
        assert_eq!(line("x\ny\u{2028}z", 100), "x yz");
    }
}
