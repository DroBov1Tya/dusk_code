//! What the user keeps to drop into a message by name: `@me` brings their profile,
//! `@name` one of their snippets. The core turns a mention into the chat's sources when
//! the message is sent.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The mention that brings the profile; no snippet can take this name.
pub const ME: &str = "me";

pub const MAX_NAME_CHARS: usize = 48;

/// A name a mention can use: ASCII letters, digits, `-` and `_`, starting with a letter
/// or digit, so it is safe as a file name and ends where punctuation starts.
pub fn is_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_CHARS
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(is_name_char)
}

pub fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

/// Each `@name` in `text` with the byte range of the whole mention. A mention starts
/// the text or follows a space or an opening bracket, so an e-mail address is not one.
pub fn find(text: &str) -> Vec<(std::ops::Range<usize>, &str)> {
    let mut found = Vec::new();
    let mut previous = None;
    for (at, c) in text.char_indices() {
        let starts =
            c == '@' && previous.is_none_or(|p: char| p.is_whitespace() || "([{".contains(p));
        previous = Some(c);
        if !starts {
            continue;
        }
        let rest = &text[at + 1..];
        let end = rest.find(|c: char| !is_name_char(c)).unwrap_or(rest.len());
        let name = &rest[..end];
        if is_name(name) {
            found.push((at..at + 1 + end, name));
        }
    }
    found
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub name: String,
    pub text: String,
}

/// Who the user is, and the files that go with `@me`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    pub about: String,
    pub files: Vec<ProfileFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProfileFile {
    pub path: PathBuf,
    /// Whether `@me` attaches it; an unmarked file stays listed for later.
    pub send: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mention_is_a_word_of_its_own() {
        let names = |text| {
            find(text)
                .into_iter()
                .map(|(_, name)| name)
                .collect::<Vec<_>>()
        };
        assert_eq!(names("@me, see @video_1."), ["me", "video_1"]);
        assert_eq!(names("mail a@b.com or (@x)"), ["x"]);
        assert_eq!(names("@ @-x @../etc"), Vec::<&str>::new());
        let text = "hi @me!";
        let (range, _) = &find(text)[0];
        assert_eq!(&text[range.clone()], "@me");
    }
}
