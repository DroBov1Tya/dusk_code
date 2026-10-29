//! Text the user pasted travels inside a message in <pasted> tags. The tags tell the model
//! what the block is, and let any client show it folded, in history as well.

const OPEN: &str = "<pasted>\n";
const CLOSE: &str = "\n</pasted>";

pub enum Part<'a> {
    Text(&'a str),
    Pasted(&'a str),
}

pub fn wrap(text: &str) -> String {
    format!("{OPEN}{text}{CLOSE}")
}

/// A message as typed text and pasted blocks, in order. An unclosed tag is plain text.
pub fn split(message: &str) -> Vec<Part<'_>> {
    let mut parts = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find(OPEN) {
        let body = &rest[start + OPEN.len()..];
        let Some(end) = body.find(CLOSE) else {
            break;
        };
        if start > 0 {
            parts.push(Part::Text(&rest[..start]));
        }
        parts.push(Part::Pasted(&body[..end]));
        rest = &body[end + CLOSE.len()..];
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
            Part::Text(text) | Part::Pasted(text) => text,
        })
        .collect()
}
