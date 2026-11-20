//! Small text forms more than one view shows: one-line text.

pub fn one_line(text: &str) -> String {
    printable(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` with every control character a program could steer the terminal with, and every
/// bidirectional control that could reorder what is drawn around it, turned into a space.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() || is_bidi(c) { ' ' } else { c })
        .collect()
}

/// Marks, embeddings, overrides and isolates: they are invisible and flip the order of
/// the text next to them (CWE-451).
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_output_cannot_steer_the_terminal() {
        let hostile = "ok\x1b[2J\x1b]0;title\x07\r\tend";
        assert!(!printable(hostile).chars().any(char::is_control));
        assert_eq!(one_line(hostile), "ok [2J ]0;title end");
    }
}
