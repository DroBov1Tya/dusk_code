//! Rust's strings and comments, found by the rules of its lexer and wiped before the
//! parser reads the file. The grammar finds them with a scanner that gives up while the
//! parser recovers from an error, and their text is then read as code: `~`, a line of
//! `///` and a line of `// Hunter2` gave the name `Hunter2`. A name cannot come from text
//! the parser was never handed, whatever it does with the rest.
//!
//! The rules are those of `rustc_lexer` in the current edition (`c"..."` and `cr"..."`
//! are strings). Where a file breaks them the reading that hides more is taken: a string
//! or a comment with no end runs to the end of the file, and a `'` that opens no whole
//! character opens nothing.

/// `source` with the text of every comment and every string replaced by spaces, byte for
/// byte: the delimiters and the line ends stay, so every place and every line is where it
/// was. A character (`'x'`) is left as written.
pub fn blank(source: &[u8]) -> Vec<u8> {
    let mut text = source.to_vec();
    let mut at = shebang(source, &mut text);
    while let Some(byte) = source.get(at) {
        let next = source.get(at + 1);
        at = match byte {
            b'/' if next == Some(&b'/') => {
                let end = find(source, at + 2, b'\n');
                wipe(&mut text, at + 2, end);
                end
            }
            b'/' if next == Some(&b'*') => {
                let end = comment_end(source, at + 2);
                wipe(&mut text, at + 2, end);
                end + 2
            }
            b'"' => {
                let end = string_end(source, at + 1);
                wipe(&mut text, at + 1, end);
                end + 1
            }
            b'\'' => after_quote(source, at + 1),
            byte if in_word(*byte) => {
                let end = word_end(source, at);
                match raw_string(source, at, end) {
                    Some((from, to, hashes)) => {
                        wipe(&mut text, from, to);
                        to + 1 + hashes
                    }
                    None => end,
                }
            }
            _ => at + 1,
        };
    }
    text
}

fn wipe(text: &mut [u8], from: usize, to: usize) {
    let hidden = text.get_mut(from..to).unwrap_or_default();
    for byte in hidden.iter_mut().filter(|byte| **byte != b'\n') {
        *byte = b' ';
    }
}

/// Where `wanted` is at or after `from`, or the end of the file.
fn find(source: &[u8], from: usize, wanted: u8) -> usize {
    let rest = source.get(from..).unwrap_or_default();
    rest.iter()
        .position(|byte| *byte == wanted)
        .map_or(source.len(), |found| from + found)
}

/// A first line `#!...` is for the shell unless it opens an attribute (`#![...]`): it is
/// wiped whole, since a quote in it would open a string that is not there. Gives where
/// the code starts.
fn shebang(source: &[u8], text: &mut [u8]) -> usize {
    if !source.starts_with(b"#!") {
        return 0;
    }
    let end = find(source, 2, b'\n');
    let line = source.get(2..end).unwrap_or_default();
    if line.trim_ascii_start().first() == Some(&b'[') {
        return 0;
    }
    wipe(text, 2, end);
    end
}

/// Where the `*/` of a block comment is, its text starting at `from`. Comments nest.
fn comment_end(source: &[u8], from: usize) -> usize {
    let (mut at, mut depth) = (from, 1_usize);
    while let Some(rest) = source.get(at..).filter(|rest| !rest.is_empty()) {
        if rest.starts_with(b"/*") {
            depth += 1;
            at += 2;
        } else if rest.starts_with(b"*/") {
            depth -= 1;
            if depth == 0 {
                return at;
            }
            at += 2;
        } else {
            at += 1;
        }
    }
    source.len()
}

/// Where the quote that closes a string is, its text starting at `from`. A backslash
/// takes the byte after it, a quote among them.
fn string_end(source: &[u8], from: usize) -> usize {
    let mut at = from;
    while let Some(byte) = source.get(at) {
        match byte {
            b'"' => return at,
            b'\\' => at += 2,
            _ => at += 1,
        }
    }
    source.len()
}

/// Where the code goes on after a `'` whose next byte is at `from`. It opens a character
/// (`'x'`, `'"'`, `'\''`, `'\u{22}'`) or it stands alone, as before the name of a lifetime
/// (`'a`), and the difference decides whether a quote after it opens a string. A character
/// is one character and a `'`, or a backslash and what follows it up to a `'`, on one line
/// and with no `"` but an escaped one. No other `'` takes anything with it: the compiler
/// reads an unclosed `'\` on into the next line, and a string there would keep its text.
fn after_quote(source: &[u8], from: usize) -> usize {
    let Some(first) = source.get(from) else {
        return from;
    };
    if *first != b'\\' {
        let one = from + char_len(*first);
        return if source.get(one) == Some(&b'\'') {
            one + 1
        } else {
            from
        };
    }
    let mut at = from;
    while let Some(byte) = source.get(at) {
        match byte {
            b'\'' => return at + 1,
            b'\n' | b'"' => break,
            b'\\' if source.get(at + 1) != Some(&b'\n') => at += 2,
            _ => at += 1,
        }
    }
    from
}

/// The raw string the word in `word..end` opens, if it does (`r"`, `br#"`, `cr##"`):
/// where its text starts, where the quote that closes it is, and how many `#` follow that
/// quote. Nothing is escaped in it; it ends at a quote with as many `#` as it opened with.
fn raw_string(source: &[u8], word: usize, end: usize) -> Option<(usize, usize, usize)> {
    if !matches!(source.get(word..end)?, b"r" | b"br" | b"cr") {
        return None;
    }
    let rest = source.get(end..)?;
    let hashes = rest.iter().take_while(|byte| **byte == b'#').count();
    if rest.get(hashes) != Some(&b'"') {
        return None;
    }
    let from = end + hashes + 1;
    let mut at = from;
    loop {
        let quote = find(source, at, b'"');
        let after = source.get(quote + 1..).unwrap_or_default();
        let closes = after.iter().take_while(|byte| **byte == b'#').count() >= hashes;
        if closes || quote == source.len() {
            return Some((from, quote, hashes));
        }
        at = quote + 1;
    }
}

/// A byte of a name, a keyword or a number. Every byte of a character past ASCII counts:
/// a name may hold one, and none of them is a quote or a slash.
fn in_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || !byte.is_ascii()
}

fn word_end(source: &[u8], from: usize) -> usize {
    let rest = source.get(from..).unwrap_or_default();
    from + rest.iter().take_while(|byte| in_word(**byte)).count()
}

/// The bytes of the character that starts with `first`.
fn char_len(first: u8) -> usize {
    match first {
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => 1,
    }
}
