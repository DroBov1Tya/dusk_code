//! Whether a model sees pictures, asked once with a square of a random colour: catalogues rarely
//! say, and a model that takes a picture without seeing it would answer from nothing.

use std::{io::Cursor, time::Duration};

use image::{ImageFormat, Rgb, RgbImage};

use super::generation::generate;
use crate::{
    providers::{ChatMessage, ChatRequest, ModelOutput, Picture, is_unsupported_pictures},
    runtime::model_choice::Choice,
};

const ASK_TIMEOUT: Duration = Duration::from_secs(60);
/// Room for a reasoning model to think before its one word.
const MAX_ANSWER_TOKENS: u64 = 512;

/// Colours the test square may have, so a guess is right one time in three.
const COLOURS: [(&str, [u8; 3]); 3] = [
    ("red", [220, 20, 20]),
    ("green", [20, 180, 40]),
    ("blue", [20, 60, 220]),
];

/// `None` when the model could not be asked, such as on a network failure.
pub async fn sees_pictures(choice: &Choice) -> Option<bool> {
    let pick = uuid::Uuid::new_v4().as_bytes()[0] as usize % COLOURS.len();
    let (colour, rgb) = COLOURS[pick];
    let messages = [
        ChatMessage::System("Answer with one word.".into()),
        ChatMessage::UserWithPictures {
            text: "What colour is this square?".into(),
            pictures: vec![square(rgb)?],
        },
    ];
    let request = ChatRequest {
        model: &choice.model.id,
        messages: &messages,
        tools: &[],
        max_output_tokens: Some(MAX_ANSWER_TOKENS),
        effort: None,
    };
    let mut output = ModelOutput::default();
    let asked = generate(choice.provider.as_ref(), request, &mut output, |_| {});
    match tokio::time::timeout(ASK_TIMEOUT, asked).await.ok()? {
        Ok(_) => {
            let answer = output.text.to_ascii_lowercase();
            let named: Vec<&str> = COLOURS
                .iter()
                .map(|(name, _)| *name)
                .filter(|name| answer.contains(name))
                .collect();
            Some(named == [colour])
        }
        Err(error) if is_unsupported_pictures(&error) => Some(false),
        Err(_) => None,
    }
}

fn square(rgb: [u8; 3]) -> Option<Picture> {
    let mut png = Cursor::new(Vec::new());
    RgbImage::from_pixel(32, 32, Rgb(rgb))
        .write_to(&mut png, ImageFormat::Png)
        .ok()?;
    use base64::{Engine, engine::general_purpose::STANDARD};
    Some(Picture {
        mime_type: "image/png",
        base64: STANDARD.encode(png.into_inner()),
    })
}
