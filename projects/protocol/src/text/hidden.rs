//! The characters that can make text read as other than it is: the one set the core and
//! every client filter untrusted text with. Each place keeps its own policy (drop, space,
//! `?`, escape, refuse); only the set is shared.

/// The black flag an emoji tag sequence (a subdivision flag such as England's) starts with.
const TAG_BASE: char = '\u{1F3F4}';
/// The tag that ends an emoji tag sequence.
const CANCEL_TAG: char = '\u{E007F}';
/// Tags a subdivision flag spells its region with: at most "gb" and a three letter code.
const MAX_FLAG_TAGS: usize = 7;

/// Characters that draw nothing, break the line or reorder what is shown: bidirectional
/// controls and marks, line and paragraph separators, zero-width and other invisible
/// format characters, and the tag block used to smuggle text past a reader.
pub fn is_hidden(c: char) -> bool {
    matches!(
        c,
        // Soft hyphen; Arabic, Syriac and Mongolian format marks.
        '\u{00AD}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            | '\u{180E}'
            // Zero width space, joiners, marks; separators; embeddings and overrides.
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            // Word joiner, invisible operators, isolates, deprecated format characters.
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

/// Hidden characters that real text needs and that hide nothing: the joiners that shape
/// Indic and Persian words and join emoji (a family, a profession), and the soft hyphen.
fn joins(c: char) -> bool {
    matches!(c, '\u{00AD}' | '\u{200C}' | '\u{200D}')
}

/// Whether `c` may be drawn as itself in a label: not a control and not hidden, except a
/// joiner or the soft hyphen.
pub fn is_plain(c: char) -> bool {
    !c.is_control() && (!is_hidden(c) || joins(c))
}

/// Whether every character of `text` is plain, the tags of a subdivision flag aside: a
/// black flag, two to seven tag letters or digits, the cancel tag.
pub fn is_plain_line(text: &str) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == TAG_BASE && chars.clone().next().is_some_and(is_flag_tag) {
            let mut tags = 0;
            loop {
                match chars.next() {
                    Some(CANCEL_TAG) if (2..=MAX_FLAG_TAGS).contains(&tags) => break,
                    Some(tag) if is_flag_tag(tag) => tags += 1,
                    _ => return false,
                }
            }
        } else if !is_plain(c) {
            return false;
        }
    }
    true
}

/// A tag a flag spells with: a tag digit or lowercase tag letter.
fn is_flag_tag(c: char) -> bool {
    matches!(c, '\u{E0030}'..='\u{E0039}' | '\u{E0061}'..='\u{E007A}')
}

#[cfg(test)]
mod tests {
    use super::*;

    const ENGLAND: &str = "\u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}";

    #[test]
    fn bidi_controls_and_marks_are_hidden() {
        for c in [
            '\u{061C}', '\u{200E}', '\u{200F}', '\u{202A}', '\u{202B}', '\u{202C}', '\u{202D}',
            '\u{202E}', '\u{2066}', '\u{2067}', '\u{2068}', '\u{2069}',
        ] {
            assert!(is_hidden(c) && !is_plain(c), "{c:?}");
        }
    }

    #[test]
    fn line_and_paragraph_separators_are_hidden() {
        for c in ['\u{2028}', '\u{2029}'] {
            assert!(is_hidden(c) && !is_plain(c), "{c:?}");
        }
    }

    #[test]
    fn zero_width_and_invisible_format_characters_are_hidden() {
        for c in [
            '\u{180E}',
            '\u{200B}',
            '\u{2060}',
            '\u{2064}',
            '\u{206A}',
            '\u{206F}',
            '\u{FEFF}',
            '\u{FFF9}',
            '\u{FFFB}',
            '\u{0600}',
            '\u{06DD}',
            '\u{070F}',
            '\u{08E2}',
            '\u{110BD}',
            '\u{13430}',
            '\u{1BCA0}',
            '\u{1D173}',
        ] {
            assert!(is_hidden(c) && !is_plain(c), "{c:?}");
        }
    }

    #[test]
    fn the_tag_block_is_hidden() {
        for c in [
            '\u{E0000}',
            '\u{E0001}',
            '\u{E0041}',
            '\u{E0067}',
            CANCEL_TAG,
        ] {
            assert!(is_hidden(c) && !is_plain(c), "{c:?}");
        }
        // Text smuggled in tags is refused, also behind a flag or without its end.
        assert!(!is_plain_line("ok\u{E0069}\u{E0067}\u{E006E}"));
        assert!(!is_plain_line("\u{1F3F4}\u{E0041}\u{E0042}\u{E007F}"));
        assert!(!is_plain_line("\u{1F3F4}\u{E0067}\u{E0062}"));
        assert!(!is_plain_line(&format!(
            "\u{1F3F4}{}\u{E007F}",
            "\u{E0061}".repeat(MAX_FLAG_TAGS + 1)
        )));
    }

    #[test]
    fn controls_are_not_plain() {
        for c in ['\0', '\n', '\r', '\t', '\u{1b}', '\u{7f}', '\u{85}'] {
            assert!(!is_plain(c), "{c:?}");
        }
    }

    #[test]
    fn real_text_stays_plain() {
        for text in [
            "Plain ASCII, with punctuation: (a) [b] {c}!",
            "Ünïcödé — עברית العربية 中文",
            // Emoji joined by zero width joiners: a family, a woman technologist.
            "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467} \u{1F469}\u{200D}\u{1F4BB}",
            // A Persian word with a zero width non-joiner, a word with a soft hyphen.
            "\u{0645}\u{06CC}\u{200C}\u{062E}\u{0648}\u{0627}\u{0647}\u{0645} Sil\u{00AD}ben",
            // A flag of a subdivision, and a keycap and skin tone.
            ENGLAND,
            "1\u{FE0F}\u{20E3} \u{1F44D}\u{1F3FD}",
        ] {
            assert!(is_plain_line(text), "{text:?}");
        }
    }
}
