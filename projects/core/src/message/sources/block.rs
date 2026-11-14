//! The block at the end of a request that tells the model about the chat's sources.

use std::path::Path;

use crate::{
    providers::{ChatMessage, Picture},
    state::{Source, estimate_tokens},
};

use super::picture::{is_picture, picture};

/// What of one source a request carries; the agent can read the rest from its file.
const MAX_ATTACHED_BYTES: usize = 256 * 1024;

/// What providers count for one picture of the size sent, roughly.
const PICTURE_TOKENS: u64 = 1500;

/// What a request adds about the sources.
pub struct SourceBlock {
    pub(super) text: String,
    pub pictures: Vec<Picture>,
    /// The sources it carries for the first time.
    pub first_time: Vec<String>,
}

impl SourceBlock {
    pub fn tokens(&self) -> u64 {
        estimate_tokens(&self.text) + self.pictures.len() as u64 * PICTURE_TOKENS
    }

    pub fn message(&self) -> ChatMessage {
        match self.pictures.is_empty() {
            true => ChatMessage::User(self.text.clone()),
            false => ChatMessage::UserWithPictures {
                text: self.text.clone(),
                pictures: self.pictures.clone(),
            },
        }
    }
}

pub(super) async fn attach(
    block: &mut SourceBlock,
    source: &Source,
    path: &Path,
    with_pictures: bool,
) {
    let (name, shown) = (&source.name, path.display());
    let Ok(bytes) = tokio::fs::read(path).await else {
        block.text.push_str(&format!("{name}: the file is gone\n"));
        return;
    };
    if is_picture(&bytes) {
        // Decoding and encoding take a while on a large photo.
        let picture = match with_pictures {
            true => tokio::task::spawn_blocking(move || picture(&bytes))
                .await
                .ok()
                .flatten(),
            false => None,
        };
        let said = match picture {
            Some(picture) => {
                block.pictures.push(picture);
                format!("{name} ({shown}): a picture, attached to this message\n")
            }
            None => format!("{name}: {shown}, a picture this model cannot see\n"),
        };
        block.text.push_str(&said);
        return;
    }
    let cut = bytes.len().min(MAX_ATTACHED_BYTES);
    // A cut can fall inside a character, which is not a sign of a binary file.
    let text = match std::str::from_utf8(&bytes[..cut]) {
        Ok(text) => text,
        Err(error) if cut < bytes.len() && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).unwrap_or_default()
        }
        Err(_) => {
            let said = format!("{name}: {shown}, a binary file that cannot be shown here\n");
            block.text.push_str(&said);
            return;
        }
    };
    let more = match cut < bytes.len() {
        true => format!("\n[cut at {cut} bytes; the rest is in {shown}]"),
        false => String::new(),
    };
    let said = format!("--- {name} ({shown})\n{text}{more}\n--- end of {name}\n");
    block.text.push_str(&said);
}
