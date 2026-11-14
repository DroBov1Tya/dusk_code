//! How pasted text and sources are marked inside a message.

const OPEN: &str = "<pasted>\n";

const CLOSE: &str = "\n</pasted>";

const SOURCE_OPEN: &str = "<source>";

const SOURCE_CLOSE: &str = "</source>";

const SOUND_OPEN: &str = "<sound>";

const SOUND_CLOSE: &str = "</sound>";

pub enum Part<'a> {
    Text(&'a str),
    Pasted(&'a str),
    /// A file kept among the chat's sources, by its name.
    Source(&'a str),
    /// A sound file the user sent: its card in the artifact store, to play, and the
    /// path it was sent as, for the model.
    Sound {
        id: &'a str,
        path: &'a str,
    },
}

pub fn wrap(text: &str) -> String {
    format!("{OPEN}{text}{CLOSE}")
}

/// How a message names a source it came with.
pub fn source_tag(name: &str) -> String {
    format!("{SOURCE_OPEN}{name}{SOURCE_CLOSE}")
}

/// How a message names a sound file it came with: the card `id` keeps a copy to play.
pub fn sound_tag(id: &str, path: &str) -> String {
    format!("{SOUND_OPEN}{id} {path}{SOUND_CLOSE}")
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
        let sound = rest
            .find(SOUND_OPEN)
            .map(|start| (start, SOUND_OPEN, SOUND_CLOSE));
        let Some((start, open, close)) = [pasted, source, sound]
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
        let inner = &body[..end];
        parts.push(match open {
            OPEN => Part::Pasted(inner),
            SOURCE_OPEN => Part::Source(inner),
            // A sound tag without its two halves is plain text.
            _ => match inner.split_once(' ') {
                Some((id, path)) => Part::Sound { id, path },
                None => Part::Text(&rest[start..start + open.len() + end + close.len()]),
            },
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
            Part::Sound { path, .. } => path,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_tag_names_its_card_and_path() {
        let message = format!("hear {} now", sound_tag("0192-ab", "/tmp/my voice.m4a"));
        let parts = split(&message);
        assert!(matches!(
            parts.as_slice(),
            [
                Part::Text("hear "),
                Part::Sound {
                    id: "0192-ab",
                    path: "/tmp/my voice.m4a"
                },
                Part::Text(" now")
            ]
        ));
        assert_eq!(plain(&message), "hear /tmp/my voice.m4a now");
        // Half a tag is the text it was.
        assert!(matches!(
            split("<sound>x</sound>").as_slice(),
            [Part::Text("<sound>x</sound>")]
        ));
        assert!(matches!(
            split("<sound>x y").as_slice(),
            [Part::Text("<sound>x y")]
        ));
    }
}
