//! Text the user pasted travels inside a message in <pasted> tags. The tags tell the model
//! what the block is, and let any client show it folded, in history as well.

const OPEN: &str = "<pasted>\n";
const CLOSE: &str = "\n</pasted>";

const SOURCE_OPEN: &str = "<source>";
const SOURCE_CLOSE: &str = "</source>";

pub enum Part<'a> {
    Text(&'a str),
    Pasted(&'a str),
    /// A file kept among the chat's sources, by its name.
    Source(&'a str),
}

pub fn wrap(text: &str) -> String {
    format!("{OPEN}{text}{CLOSE}")
}

/// How a message names a source it came with.
pub fn source_tag(name: &str) -> String {
    format!("{SOURCE_OPEN}{name}{SOURCE_CLOSE}")
}

/// A message as typed text, pasted blocks and sources, in order. An unclosed tag is
/// plain text.
pub fn split(message: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = message;
    loop {
        let pasted = rest.find(OPEN).map(|start| (start, OPEN, CLOSE));
        let source = rest
            .find(SOURCE_OPEN)
            .map(|start| (start, SOURCE_OPEN, SOURCE_CLOSE));
        let Some((start, open, close)) = [pasted, source]
            .into_iter()
            .flatten()
            .min_by_key(|(start, ..)| *start)
        else {
            break;
        };
        let body = &rest[start + open.len()..];
        let Some(end) = body.find(close) else {
            break;
        };
        if start > 0 {
            parts.push(Part::Text(&rest[..start]));
        }
        parts.push(match open {
            OPEN => Part::Pasted(&body[..end]),
            _ => Part::Source(&body[..end]),
        });
        rest = &body[end + close.len()..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest));
    }
    parts
}

pub fn has_pasted(message: &str) -> bool {
    split(message)
        .iter()
        .any(|part| matches!(part, Part::Pasted(_)))
}

/// The message without tags: what the user pasted, where they pasted it.
pub fn plain(message: &str) -> String {
    split(message)
        .into_iter()
        .map(|part| match part {
            Part::Text(text) | Part::Pasted(text) | Part::Source(text) => text,
        })
        .collect()
}
