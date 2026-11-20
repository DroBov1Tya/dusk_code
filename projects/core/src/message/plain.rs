//! Text a tool or a model wrote, made plain before the core quotes it in a line of its
//! own or keeps it for a later turn: a stub of a stored result, an episode. Nothing in
//! it can then end the line, close a bracket of the core's, or hide from the reader.

use dusk_protocol::text::is_hidden;

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
            c if c.is_control() || is_hidden(c) => ' ',
            c => c,
        })
        .collect();
    mapped.trim().to_owned()
}
