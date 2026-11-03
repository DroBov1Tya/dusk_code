//! Small text forms more than one view shows: durations and one-line text.

/// `mm:ss`, or `h:mm:ss` from an hour on.
pub fn format_duration(ms: u64) -> String {
    let seconds = ms / 1_000;
    let minutes = seconds / 60;
    if minutes >= 60 {
        format!("{}:{:02}:{:02}", minutes / 60, minutes % 60, seconds % 60)
    } else {
        format!("{minutes:02}:{:02}", seconds % 60)
    }
}

pub fn one_line(text: &str) -> String {
    printable(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `text` with every control character a program could steer the terminal with turned
/// into a space.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
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
