//! Coloured labels inside a message's text, written `{{tone: label}}`. The tone is one of
//! a fixed set each client maps to its theme; the model never names a colour.

use crate::thread::block::is_plain;

/// How long a label may be; a longer one stays the text it was written as.
pub const MAX_MARK_CHARS: usize = 40;

/// What a label says about its text. A client gives each a colour of its theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
    Accent,
}

impl Tone {
    pub const ALL: [Tone; 6] = [
        Self::Neutral,
        Self::Info,
        Self::Success,
        Self::Warning,
        Self::Danger,
        Self::Accent,
    ];

    /// The name the model writes.
    pub fn name(self) -> &'static str {
        match self {
            Self::Neutral => "neutral",
            Self::Info => "info",
            Self::Success => "success",
            Self::Warning => "warning",
            Self::Danger => "danger",
            Self::Accent => "accent",
        }
    }
}

/// A piece of a message's text: as written, or a label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span<'a> {
    Text(&'a str),
    /// `text` is plain: a client draws it as it is, never as markdown or a link.
    Mark {
        tone: Tone,
        text: &'a str,
    },
}

/// Splits text into what is written and the labels in it. Anything that is not exactly a
/// label stays text, so nothing is lost. A client calls it on the text runs its markdown
/// parser gives, never on code; a label has no markdown characters, so a run holds it whole.
pub fn marks(text: &str) -> Vec<Span<'_>> {
    let mut spans = Vec::new();
    // Where the text not yet given out starts, and where the search goes on.
    let (mut written, mut at) = (0, 0);
    while let Some(found) = text[at..].find("{{") {
        let open = at + found;
        let Some((tone, label, len)) = mark_at(&text[open..]) else {
            at = open + 1;
            continue;
        };
        if written < open {
            spans.push(Span::Text(&text[written..open]));
        }
        spans.push(Span::Mark { tone, text: label });
        at = open + len;
        written = at;
    }
    if written < text.len() {
        spans.push(Span::Text(&text[written..]));
    }
    spans
}

/// The text with each label replaced by its words, for where labels are not drawn.
pub fn unmarked(text: &str) -> String {
    marks(text)
        .into_iter()
        .map(|span| match span {
            Span::Text(text) | Span::Mark { text, .. } => text,
        })
        .collect()
}

/// The label `text` starts with: its tone, its words and how many bytes it takes. It looks
/// at most `MAX_MARK_CHARS` characters ahead, so a text full of `{{` is still read once.
fn mark_at(text: &str) -> Option<(Tone, &str, usize)> {
    let rest = text.strip_prefix("{{")?;
    let (tone, rest) = Tone::ALL.into_iter().find_map(|tone| {
        let rest = rest.strip_prefix(tone.name())?.strip_prefix(':')?;
        Some((tone, rest))
    })?;
    let rest = rest.strip_prefix(' ').unwrap_or(rest);
    let mut end = None;
    for (count, (index, c)) in rest.char_indices().enumerate() {
        if c == '}' {
            end = rest[index..].starts_with("}}").then_some(index);
            break;
        }
        if count == MAX_MARK_CHARS || !in_label(c) {
            break;
        }
    }
    let label = &rest[..end?];
    if label.is_empty() || label != label.trim() {
        return None;
    }
    let len = text.len() - rest.len() + label.len() + 2;
    Some((tone, label, len))
}

/// Plain, and not a character markdown gives a meaning: a label cannot open a link, a
/// tag, emphasis or code, and cannot hold another label.
fn in_label(c: char) -> bool {
    is_plain(c)
        && !matches!(
            c,
            '{' | '}' | '`' | '*' | '_' | '[' | ']' | '<' | '>' | '\\'
        )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn mark(tone: Tone, text: &str) -> Span<'_> {
        Span::Mark { tone, text }
    }

    #[test]
    fn labels_are_found_and_the_rest_stays_text() {
        assert_eq!(
            marks("Build {{success: Passed}}, lint {{danger:3 failed}}."),
            [
                Span::Text("Build "),
                mark(Tone::Success, "Passed"),
                Span::Text(", lint "),
                mark(Tone::Danger, "3 failed"),
                Span::Text("."),
            ]
        );
        assert_eq!(unmarked("a {{info: b}} c {{red: d}}"), "a b c {{red: d}}");
        assert!(marks("").is_empty());
    }

    #[test]
    fn what_is_not_exactly_a_label_stays_text() {
        let limit = "x".repeat(MAX_MARK_CHARS);
        let fits = format!("{{{{info: {limit}}}}}");
        assert_eq!(marks(&fits), [mark(Tone::Info, &limit)]);
        let over = format!("{{{{info: {limit}x}}}}");
        assert_eq!(marks(&over), [Span::Text(&over)]);
        for text in [
            "{{red: x}}",
            "{{#ff0000: x}}",
            "{{Success: x}}",
            "{{success x}}",
            "{{success:}}",
            "{{success: }}",
            "{{success:  x}}",
            "{{success: x }}",
            "{{success: x}",
            "{{success: a\nb}}",
            "{{success: [a](http://b)}}",
            "{{success: <b>a</b>}}",
            "{{success: `a`}}",
            "{{success: a_b}}",
            "{{success: a\u{202e}b}}",
            "{{success: {{info: a}}",
        ] {
            let spans = marks(text);
            let kept = match text {
                // The inner label is a label; the outer one is not.
                "{{success: {{info: a}}" => {
                    vec![Span::Text("{{success: "), mark(Tone::Info, "a")]
                }
                _ => vec![Span::Text(text)],
            };
            assert_eq!(spans, kept, "{text}");
        }
    }

    /// Text built from the pieces labels are made of, so that near-labels are common.
    fn text() -> impl Strategy<Value = String> {
        let piece = prop_oneof![
            Just("{{".to_owned()),
            Just("}}".to_owned()),
            Just("{".to_owned()),
            Just("}".to_owned()),
            Just(":".to_owned()),
            Just(": ".to_owned()),
            Just(" ".to_owned()),
            Just("\n".to_owned()),
            tone().prop_map(str::to_owned),
            // Whole labels and near ones: about the cap long, with characters a label refuses.
            (tone(), "[ -~]{0,42}").prop_map(|(tone, label)| format!("{{{{{tone}: {label}}}}}")),
            (tone(), "[a-z0-9 ]{1,8}").prop_map(|(tone, label)| format!("{{{{{tone}:{label}}}}}")),
            "[a-z*_\\[<]{0,6}",
            "\\PC{0,45}",
            any::<String>(),
        ];
        prop::collection::vec(piece, 0..24).prop_map(|pieces| pieces.concat())
    }

    fn tone() -> impl Strategy<Value = &'static str> {
        prop::sample::select(Tone::ALL.to_vec()).prop_map(Tone::name)
    }

    fn offset(whole: &str, part: &str) -> usize {
        part.as_ptr() as usize - whole.as_ptr() as usize
    }

    proptest! {
        /// The spans cover the input in order; the only bytes outside them are the
        /// braces, tone name, colon and one space of a label.
        #[test]
        fn no_text_is_lost_and_a_label_is_only_a_tone_and_plain_text(input in text()) {
            let mut at = 0;
            for span in marks(&input) {
                match span {
                    Span::Text(text) => {
                        prop_assert!(!text.is_empty());
                        prop_assert_eq!(offset(&input, text), at);
                        at += text.len();
                    }
                    Span::Mark { tone, text } => {
                        prop_assert!(Tone::ALL.contains(&tone));
                        let start = offset(&input, text);
                        let opening = &input[at..start];
                        let bare = format!("{{{{{}:", tone.name());
                        let spaced = format!("{bare} ");
                        prop_assert!(opening == bare || opening == spaced, "{}", opening);
                        let end = start + text.len();
                        prop_assert_eq!(&input[end..end + 2], "}}");
                        prop_assert!(!text.is_empty() && text == text.trim());
                        prop_assert!(text.chars().count() <= MAX_MARK_CHARS);
                        prop_assert!(text.chars().all(in_label));
                        at = end + 2;
                    }
                }
            }
            prop_assert_eq!(at, input.len());
        }
    }
}
