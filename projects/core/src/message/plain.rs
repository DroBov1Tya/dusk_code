//! Text a tool or a model wrote, made plain before the core quotes it in a line of its
//! own or keeps it for a later turn: a stub of a stored result, an episode. Nothing in
//! it can then end the line, close a bracket of the core's, or hide from the reader.

/// `text` on one line: control and invisible characters become spaces, brackets and
/// braces parentheses, double quotes single ones; the ends are trimmed. The brackets,
/// braces and double quotes around it are then the core's own.
pub fn plain(text: &str) -> String {
    let mapped: String = text
        .chars()
        .map(|c| match c {
            '"' => '\'',
            '[' | '{' => '(',
            ']' | '}' => ')',
            c if c.is_control() || hidden(c) => ' ',
            c => c,
        })
        .collect();
    mapped.trim().to_owned()
}

/// Characters that show nothing or reorder what is shown: bidirectional controls, line
/// and paragraph separators, zero-width marks and the tag block used to smuggle text.
pub fn hidden(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{2028}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
            | '\u{E0000}'..='\u{E007F}'
    )
}
