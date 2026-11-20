//! The characters that can make text read as other than it is: the one set the core and
//! every client filter untrusted text with. Each place keeps its own policy (drop, space,
//! `?`, escape, refuse); only the set is shared.

/// The black flag an emoji tag sequence (a subdivision flag such as England's) starts with.
const TAG_BASE: char = '\u{1F3F4}';
/// The tag that ends an emoji tag sequence.
const CANCEL_TAG: char = '\u{E007F}';
/// Tags a subdivision flag spells its region with: two letters of a country and one to
/// three letters or digits of the region, such as "gbsct".
const MIN_FLAG_TAGS: usize = 3;
const MAX_FLAG_TAGS: usize = 5;

/// Characters that draw nothing, break the line or reorder what is shown: bidirectional
/// controls and marks, line and paragraph separators, zero-width and other invisible
/// format characters, fillers that draw as blank, variation selectors that can hide bytes
/// after a visible character, and the tag block used to smuggle text past a reader. The
/// emoji and text presentation selectors (FE0E, FE0F) are kept: emoji need them.
pub fn is_hidden(c: char) -> bool {
    matches!(
        c,
        // Soft hyphen; Arabic, Syriac and Mongolian format marks.
        '\u{00AD}'
            // Combining grapheme joiner.
            | '\u{034F}'
            | '\u{0600}'..='\u{0605}'
            | '\u{061C}'
            | '\u{06DD}'
            | '\u{070F}'
            | '\u{0890}'..='\u{0891}'
            | '\u{08E2}'
            // Hangul fillers, Khmer inherent vowels, Mongolian variation selectors.
            | '\u{115F}'..='\u{1160}'
            | '\u{17B4}'..='\u{17B5}'
            | '\u{180B}'..='\u{180F}'
            // Zero width space, joiners, marks; separators; embeddings and overrides.
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            // Word joiner, invisible operators, isolates, deprecated format characters.
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FE00}'..='\u{FE0D}'
            | '\u{FEFF}'
            | '\u{FFA0}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{110BD}'
            | '\u{110CD}'
            | '\u{13430}'..='\u{1343F}'
            | '\u{1BCA0}'..='\u{1BCA3}'
            | '\u{1D173}'..='\u{1D17A}'
            // Tags, variation selectors supplement, and the rest of plane 14 kept unassigned.
            | '\u{E0000}'..='\u{E0FFF}'
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
/// black flag, two tag letters, one to three tag letters or digits, the cancel tag.
pub fn is_plain_line(text: &str) -> bool {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == TAG_BASE && chars.clone().next().is_some_and(is_tag_letter) {
            let mut tags = 0;
            loop {
                match chars.next() {
                    Some(CANCEL_TAG) if (MIN_FLAG_TAGS..=MAX_FLAG_TAGS).contains(&tags) => break,
                    Some(tag) if tags < 2 && is_tag_letter(tag) => tags += 1,
                    Some(tag) if (2..MAX_FLAG_TAGS).contains(&tags) && is_flag_tag(tag) => {
                        tags += 1
                    }
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
    matches!(c, '\u{E0030}'..='\u{E0039}') || is_tag_letter(c)
}

/// A lowercase tag letter.
fn is_tag_letter(c: char) -> bool {
    matches!(c, '\u{E0061}'..='\u{E007A}')
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
            // Fillers and selectors that draw nothing after a visible character.
            '\u{034F}',
            '\u{115F}',
            '\u{1160}',
            '\u{3164}',
            '\u{FFA0}',
            '\u{17B4}',
            '\u{17B5}',
            '\u{180B}',
            '\u{180F}',
            '\u{FE00}',
            '\u{FE0D}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{E0FFF}',
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
        // Only the form of a subdivision flag: two letters, then one to three of a region.
        assert!(!is_plain_line("\u{1F3F4}\u{E0067}\u{E0062}\u{E007F}"));
        assert!(!is_plain_line(
            "\u{1F3F4}\u{E0031}\u{E0062}\u{E0065}\u{E007F}"
        ));
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
            "1\u{FE0F}\u{20E3} \u{1F44D}\u{1F3FD} \u{2764}\u{FE0E}",
        ] {
            assert!(is_plain_line(text), "{text:?}");
        }
    }
}
