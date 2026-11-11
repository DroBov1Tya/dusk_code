//! The boxes of the two forms.

use dusk_protocol::config::VoiceSettings;
use gpui_kit::{component::input::InputState, *};

/// The boxes of the two forms.
pub struct VoiceForm {
    pub(super) stt_url: Entity<InputState>,
    pub(super) stt_model: Entity<InputState>,
    pub(super) stt_key: Entity<InputState>,
    pub(super) tts_url: Entity<InputState>,
    pub(super) tts_model: Entity<InputState>,
    pub(super) tts_voice: Entity<InputState>,
    pub(super) tts_key: Entity<InputState>,
    /// Settings that arrived and are to be put in the boxes when next drawn.
    pub(super) fill: Option<VoiceSettings>,
    /// The key boxes are emptied when next drawn.
    pub(super) clear_keys: bool,
}

impl VoiceForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let field = |hint: &'static str, masked: bool, window: &mut Window, cx: &mut App| {
            cx.new(|cx| InputState::new(window, cx).placeholder(hint).masked(masked))
        };
        Self {
            stt_url: field("https://host/v1", false, window, cx),
            stt_model: field("whisper-1", false, window, cx),
            stt_key: field("Paste the key", true, window, cx),
            tts_url: field("https://host/v1", false, window, cx),
            tts_model: field("tts-1", false, window, cx),
            tts_voice: field("alloy", false, window, cx),
            tts_key: field("Paste the key", true, window, cx),
            fill: None,
            clear_keys: false,
        }
    }

    /// Puts arrived settings and cleared keys into the boxes, which takes the window.
    pub fn settle(&mut self, window: &mut Window, cx: &mut App) {
        let set = |input: &Entity<InputState>, text: &str, window: &mut Window, cx: &mut App| {
            input.update(cx, |input, cx| input.set_value(text, window, cx));
        };
        if let Some(voice) = self.fill.take() {
            let stt = voice.stt.unwrap_or_default();
            let tts = voice.tts.unwrap_or_default();
            set(&self.stt_url, &stt.base_url, window, cx);
            set(&self.stt_model, &stt.model, window, cx);
            set(&self.tts_url, &tts.base_url, window, cx);
            set(&self.tts_model, &tts.model, window, cx);
            set(
                &self.tts_voice,
                tts.voice.as_deref().unwrap_or_default(),
                window,
                cx,
            );
        }
        if std::mem::take(&mut self.clear_keys) {
            set(&self.stt_key, "", window, cx);
            set(&self.tts_key, "", window, cx);
        }
    }
}
