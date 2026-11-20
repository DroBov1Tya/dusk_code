//! Model text shown as a single plain line.

/// Longest reason kept, in characters.
pub const MAX_REASON_CHARS: usize = 200;

/// The first line of `text` as plain data: control characters and the characters that
/// hide or reorder what a reader sees (`dusk_protocol::text::is_plain`) removed, trimmed, cut at `max_chars`.
pub fn plain_line(text: &str, max_chars: usize) -> String {
    let first = text.split(['\n', '\r']).next().unwrap_or("");
    let clean: String = first.chars().filter(|c| plain_char(*c, false)).collect();
    clean.trim().chars().take(max_chars).collect()
}

/// One line of skill text as plain data, as `plain_line` but a tab stays and nothing is
/// trimmed, so a diff keeps the file's indentation.
pub fn plain_diff_line(line: &str) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    line.chars().filter(|c| plain_char(*c, true)).collect()
}

fn plain_char(c: char, keep_tab: bool) -> bool {
    (keep_tab && c == '\t') || dusk_protocol::text::is_plain(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_line_strips_and_caps() {
        let reason = format!(
            "\u{1b}[31mfix\u{202e}e\u{061c}d\nsecond line{}",
            "y".repeat(10_000)
        );
        let line = plain_line(&reason, MAX_REASON_CHARS);
        assert!(
            !line.contains('\u{1b}')
                && !line.contains('\u{202e}')
                && !line.contains('\u{061c}')
                && !line.contains('\n')
        );
        assert!(line.starts_with("[31mfixed"));
        assert!(!line.contains("second"));
        assert!(line.chars().count() <= 200);
    }
}
