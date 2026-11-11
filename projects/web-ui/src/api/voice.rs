//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    client::{Request, Response, VoiceRequest},
    config::VoiceModels,
};

use super::*;

pub async fn voice_models() -> Res<VoiceModels> {
    ask!(Request::Voice(VoiceRequest::Models), Response::VoiceModels(m) => m)
}

pub async fn voice(call: VoiceRequest) -> Res<()> {
    done(Request::Voice(call)).await
}

pub async fn transcribe(audio: Vec<u8>) -> Res<String> {
    ask!(Request::Voice(VoiceRequest::Transcribe { audio }), Response::Text(t) => t)
}

pub async fn speak(text: String) -> Res<dusk_protocol::tools::Card> {
    ask!(Request::Voice(VoiceRequest::Speak { text }), Response::Card(c) => c)
}
