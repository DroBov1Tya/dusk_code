//! The browser's microphone and speakers: a recording becomes a 16 kHz WAV for the core's
//! speech recognition, and a spoken answer is a sound card the page plays.

use crate::{api, state::state};
use dusk_protocol::client::wav;
use leptos::prelude::*;
use std::{cell::RefCell, rc::Rc};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AudioContext, AudioProcessingEvent, HtmlAudioElement, MediaStream, MediaStreamAudioSourceNode,
    MediaStreamConstraints, MediaStreamTrack, ScriptProcessorNode,
};

/// What the core's recognizer wants, and a limit that keeps the request frame small.
const RATE: u32 = 16_000;
pub const MAX_SECONDS: usize = 120;

pub fn speaking() -> bool {
    state().speaking.get()
}

/// Reads `text` aloud: the core makes the sound, and it plays at once.
pub fn speak(text: String) {
    let app = state();
    if app.speaking.get_untracked() {
        return;
    }
    app.speaking.set(true);
    leptos::task::spawn_local(async move {
        let played = async {
            let card = api::speak(text).await?;
            let audio = HtmlAudioElement::new_with_src(&format!("/api/card/{}/audio", card.id))
                .map_err(js_error)?;
            let finished = Closure::once_into_js(move || app.speaking.set(false));
            audio.set_onended(Some(finished.unchecked_ref()));
            JsFuture::from(audio.play().map_err(js_error)?)
                .await
                .map_err(js_error)?;
            Ok::<(), String>(())
        };
        if let Err(error) = played.await {
            app.speaking.set(false);
            app.fail(error);
        }
    });
}

/// A recording in progress.
pub struct Recording {
    context: AudioContext,
    stream: MediaStream,
    source: MediaStreamAudioSourceNode,
    processor: ScriptProcessorNode,
    samples: Rc<RefCell<Vec<f32>>>,
    _callback: Closure<dyn FnMut(AudioProcessingEvent)>,
}

pub async fn start_recording() -> Result<Recording, String> {
    let window = web_sys::window().ok_or("no window")?;
    let devices = window.navigator().media_devices().map_err(js_error)?;
    let constraints = MediaStreamConstraints::new();
    constraints.set_audio(&JsValue::TRUE);
    let promise = devices
        .get_user_media_with_constraints(&constraints)
        .map_err(js_error)?;
    let stream: MediaStream = JsFuture::from(promise)
        .await
        .map_err(|_| "The microphone was not allowed.".to_owned())?
        .dyn_into()
        .map_err(js_error)?;
    let context = AudioContext::new().map_err(js_error)?;
    let source = context
        .create_media_stream_source(&stream)
        .map_err(js_error)?;
    let processor = context
        .create_script_processor_with_buffer_size_and_number_of_input_channels_and_number_of_output_channels(4096, 1, 1)
        .map_err(js_error)?;
    let samples = Rc::new(RefCell::new(Vec::new()));
    let limit = context.sample_rate() as usize * MAX_SECONDS;
    let callback = {
        let samples = Rc::clone(&samples);
        Closure::<dyn FnMut(AudioProcessingEvent)>::new(move |event: AudioProcessingEvent| {
            let Ok(buffer) = event.input_buffer() else {
                return;
            };
            if let Ok(chunk) = buffer.get_channel_data(0) {
                let mut samples = samples.borrow_mut();
                if samples.len() < limit {
                    samples.extend_from_slice(&chunk);
                }
            }
        })
    };
    processor.set_onaudioprocess(Some(callback.as_ref().unchecked_ref()));
    source
        .connect_with_audio_node(&processor)
        .map_err(js_error)?;
    processor
        .connect_with_audio_node(&context.destination())
        .map_err(js_error)?;
    Ok(Recording {
        context,
        stream,
        source,
        processor,
        samples,
        _callback: callback,
    })
}

impl Recording {
    /// Ends the recording and returns it as a WAV file.
    pub fn finish(self) -> Vec<u8> {
        self.processor.set_onaudioprocess(None);
        let _ = self.source.disconnect();
        let _ = self.processor.disconnect();
        for track in self.stream.get_tracks().iter() {
            if let Ok(track) = track.dyn_into::<MediaStreamTrack>() {
                track.stop();
            }
        }
        let rate = self.context.sample_rate() as u32;
        let _ = self.context.close();
        wav(&downsample(&self.samples.borrow(), rate), RATE)
    }
}

/// Mono samples at `from` Hz as 16 kHz samples, by averaging each window.
fn downsample(samples: &[f32], from: u32) -> Vec<f32> {
    let ratio = from as f64 / RATE as f64;
    let count = (samples.len() as f64 / ratio) as usize;
    (0..count)
        .map(|i| {
            let start = (i as f64 * ratio) as usize;
            let end = (((i + 1) as f64 * ratio) as usize).clamp(start + 1, samples.len());
            let window = &samples[start..end];
            window.iter().sum::<f32>() / window.len() as f32
        })
        .collect()
}

fn js_error(error: JsValue) -> String {
    error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &"message".into())
                .ok()?
                .as_string()
        })
        .unwrap_or_else(|| "the browser refused".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downsampling_keeps_the_length_in_seconds() {
        let one_second = vec![0.5f32; 48_000];
        assert_eq!(downsample(&one_second, 48_000).len(), 16_000);
    }
}
