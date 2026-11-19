//! A pinned message: one the user keeps a click away. It is for the person only: nothing
//! of it reaches the model, and a pin keeps nothing from a compaction.

use serde::{Deserialize, Serialize};

use crate::thread::{Line, MessageRole, Part, block::is_plain, split, unmarked};

/// The most pins a thread keeps.
pub const MAX_PINS: usize = 8;
/// The longest excerpt of a pin.
pub const PIN_CHARS: usize = 120;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
    pub message_id: String,
    /// Order within the thread, from 1.
    pub sequence: u64,
    /// `User` or `Assistant`.
    pub role: MessageRole,
    /// The start of the message as one line of plain text, cut by the core.
    pub excerpt: Line<PIN_CHARS>,
    /// Unix milliseconds.
    pub pinned_at: i64,
    /// A compaction summed the message up: it is still in the history, no longer in what
    /// the model is sent.
    #[serde(default)]
    pub compacted: bool,
}

/// The start of `text` as one plain line of at most `PIN_CHARS` characters: pastes and
/// files as their names, labels as their words, markdown marks and line breaks gone, an
/// answer to the agent's question as the answer alone. Never empty.
pub fn excerpt(text: &str) -> Line<PIN_CHARS> {
    let text = match text
        .strip_prefix("QUESTION: ")
        .and_then(|rest| rest.split_once("\nANSWER: "))
    {
        Some((_, answer)) => answer,
        None => text,
    };
    let joined: String = split(text)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) => unmarked(text),
            Part::Pasted(_) => " [paste] ".to_owned(),
            Part::Source(name) => format!(" [{name}] "),
            Part::Sound { .. } => " [sound] ".to_owned(),
        })
        .collect();
    let words: Vec<&str> = joined
        .split(|c: char| c.is_whitespace() || !is_plain(c))
        .map(|word| word.trim_matches(['#', '*', '_', '`', '>', '~']))
        .filter(|word| !word.is_empty())
        .collect();
    let line = words.join(" ");
    let line = match line.chars().count() > PIN_CHARS {
        true => {
            let kept: String = line.chars().take(PIN_CHARS - 1).collect();
            format!("{}\u{2026}", kept.trim_end())
        }
        false => line,
    };
    Line::new(&line).unwrap_or_else(|_| Line::new("\u{2026}").expect("a plain line"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_excerpt_is_one_plain_capped_line() {
        let text = format!(
            "## Title\n\n**Use** `rustls`\u{202e} only\n{}",
            "word ".repeat(80)
        );
        let line = excerpt(&text);
        assert!(line.as_str().starts_with("Title Use rustls only word"));
        assert_eq!(line.as_str().chars().count(), PIN_CHARS);
        assert!(line.as_str().ends_with('\u{2026}'));
        assert_eq!(excerpt("  \n ").as_str(), "\u{2026}");
    }
}
