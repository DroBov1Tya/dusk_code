use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};
use dusk_protocol::config::{VoiceEndpoint, VoiceSettings};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    *,
};

mod form;
mod setup;

pub use form::VoiceForm;
pub use setup::{Speech, missing};

const RECOGNITION: [(&str, &str); 3] = [
    (
        "Qwen3-ASR",
        "https://huggingface.co/models?search=Qwen3-ASR",
    ),
    (
        "Whisper large-v3-turbo",
        "https://huggingface.co/openai/whisper-large-v3-turbo",
    ),
    (
        "Parakeet TDT 0.6B v3",
        "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3",
    ),
];

const SYNTHESIS: [(&str, &str); 3] = [
    (
        "Qwen3-TTS",
        "https://huggingface.co/models?search=Qwen3-TTS",
    ),
    ("VoxCPM2", "https://huggingface.co/openbmb/VoxCPM2"),
    ("Kokoro-82M", "https://huggingface.co/hexgrad/Kokoro-82M"),
];

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let form = &ws.voice_form;
    let state = |endpoint: Option<&VoiceEndpoint>| match endpoint {
        Some(endpoint) => format!("Set up: {} at {}", endpoint.model, endpoint.base_url),
        None => "Not set up".to_owned(),
    };
    let current = ws.voice.clone().unwrap_or_default();
    let mut page = div().flex().flex_col().gap_3().max_w(px(720.));
    page = page.child(div().small().text_color(muted).child(
        "Speech goes through any server that speaks the OpenAI audio API: a hosted account, \
         or a model run on this machine. Keys are kept in the system keychain.",
    ));
    page = page.child(
        div()
            .flex()
            .flex_row()
            .gap_2()
            .child(preset(
                "voice-preset-openai",
                "OpenAI",
                "https://api.openai.com/v1",
                ("whisper-1", "tts-1", "alloy"),
                cx,
            ))
            .child(preset(
                "voice-preset-local",
                "This machine, port 8000",
                "http://127.0.0.1:8000/v1",
                ("", "", ""),
                cx,
            )),
    );
    page = page
        .child(widgets::caption("Speech recognition: the microphone", cx).px_0())
        .child(
            div()
                .small()
                .text_color(muted)
                .child(state(current.stt.as_ref())),
        )
        .child(Input::new(&form.stt_url))
        .child(Input::new(&form.stt_model))
        .child(Input::new(&form.stt_key))
        .child(widgets::caption("Speech synthesis: read aloud", cx).px_0())
        .child(
            div()
                .small()
                .text_color(muted)
                .child(state(current.tts.as_ref())),
        )
        .child(Input::new(&form.tts_url))
        .child(Input::new(&form.tts_model))
        .child(Input::new(&form.tts_voice))
        .child(Input::new(&form.tts_key))
        .child(
            div().flex().flex_row().child(
                Button::new("voice-save")
                    .small()
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(|ws, _, _, cx| ws.save_voice(cx))),
            ),
        );
    page = page
        .child(widgets::caption("Models worth trying", cx).px_0())
        .child(div().small().text_color(muted).child(
            "Run one behind a server that speaks the OpenAI audio API, then point the boxes \
             above at it. Pages open on Hugging Face; a token for gated models is set in \
             Settings, Secrets.",
        ));
    for (title, items) in [("Recognition", RECOGNITION), ("Synthesis", SYNTHESIS)] {
        let mut row = div().flex().flex_row().flex_wrap().items_center().gap_2();
        row = row.child(div().small().text_color(muted).child(title));
        for (name, url) in items {
            row = row.child(
                Button::new(SharedString::from(format!("voice-model-{name}")))
                    .small()
                    .ghost()
                    .label(name)
                    .tooltip("Open on Hugging Face")
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
            );
        }
        page = page.child(row);
    }
    page
}

/// A button that fills the boxes with a known server's address and models.
fn preset(
    id: &'static str,
    label: &'static str,
    url: &'static str,
    (stt, tts, voice): (&'static str, &'static str, &'static str),
    cx: &mut Context<Workspace>,
) -> Button {
    Button::new(id)
        .small()
        .label(label)
        .on_click(cx.listener(move |ws, _, _, cx| {
            let endpoint = |model: &str, voice: &str| VoiceEndpoint {
                base_url: url.to_owned(),
                model: model.to_owned(),
                voice: (!voice.is_empty()).then(|| voice.to_owned()),
            };
            ws.voice_form.fill = Some(VoiceSettings {
                stt: Some(endpoint(stt, "")),
                tts: Some(endpoint(tts, voice)),
            });
            cx.notify();
        }))
}
