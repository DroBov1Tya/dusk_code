//! A message as it is read aloud.

use dusk_protocol::config::MAX_SPEECH_CHARS;

/// A message as it is read aloud: code blocks are left out, links say their words, and the
/// marks of Markdown go, so the voice does not spell them. Cut at the limit.
pub fn speech_text(markdown: &str) -> String {
    let mut spoken = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let line = line.trim_start_matches(['#', '>', ' ']);
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        spoken.push_str(&plain_line(line));
        spoken.push('\n');
    }
    let spoken: String = spoken.split_whitespace().collect::<Vec<_>>().join(" ");
    spoken.chars().take(MAX_SPEECH_CHARS).collect()
}

/// A line without emphasis marks, inline code ticks and link targets.
pub(super) fn plain_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '_' | '`' | '~' => {}
            // `[words](target)` reads as its words.
            ']' if chars.peek() == Some(&'(') => {
                for skipped in chars.by_ref() {
                    if skipped == ')' {
                        break;
                    }
                }
            }
            '[' => {}
            _ => out.push(c),
        }
    }
    out
}
