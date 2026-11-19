//! Model text shown as a single plain line.

/// Longest reason kept, in characters.
// Used by the skill-evolve tool (Task 4); until then only tests call these.
#[allow(dead_code)]
pub const MAX_REASON_CHARS: usize = 200;

/// The first line of `text` as plain data: control characters and bidi marks (which can
/// reorder what a reader sees) removed, trimmed, cut at `max_chars`.
#[allow(dead_code)]
pub fn plain_line(text: &str, max_chars: usize) -> String {
    let first = text.split(['\n', '\r']).next().unwrap_or("");
    let clean: String = first
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}')
        })
        .collect();
    clean.trim().chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_line_strips_and_caps() {
        let reason = format!("\u{1b}[31mfix\u{202e}ed\nsecond line{}", "y".repeat(10_000));
        let line = plain_line(&reason, MAX_REASON_CHARS);
        assert!(!line.contains('\u{1b}') && !line.contains('\u{202e}') && !line.contains('\n'));
        assert!(line.starts_with("[31mfixed"));
        assert!(!line.contains("second"));
        assert!(line.chars().count() <= 200);
    }
}
