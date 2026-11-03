//! Whether a model sees pictures, asked once with a small red square: catalogues rarely
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

/// `None` when the model could not be asked, such as on a network failure.
pub async fn sees_pictures(choice: &Choice) -> Option<bool> {
    let messages = [
        ChatMessage::System("Answer with one word.".into()),
        ChatMessage::UserWithPictures {
            text: "What colour is this square?".into(),
            pictures: vec![red_square()?],
        },
    ];
    let request = ChatRequest {
        model: &choice.model.id,
        messages: &messages,
        tools: &[],
        max_output_tokens: MAX_ANSWER_TOKENS,
        effort: None,
    };
    let mut output = ModelOutput::default();
    let asked = generate(choice.provider.as_ref(), request, &mut output, |_| {});
    match tokio::time::timeout(ASK_TIMEOUT, asked).await.ok()? {
        Ok(_) => Some(output.text.to_ascii_lowercase().contains("red")),
        Err(error) if is_unsupported_pictures(&error) => Some(false),
        Err(_) => None,
    }
}

fn red_square() -> Option<Picture> {
    let mut png = Cursor::new(Vec::new());
    RgbImage::from_pixel(32, 32, Rgb([220, 20, 20]))
        .write_to(&mut png, ImageFormat::Png)
        .ok()?;
    use base64::{Engine, engine::general_purpose::STANDARD};
    Some(Picture {
        mime_type: "image/png",
        base64: STANDARD.encode(png.into_inner()),
    })
}
