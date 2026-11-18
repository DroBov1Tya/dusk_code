//! The Voice tab: the two server cards, where to start, and the models to try.

use crate::ui::press::Press;
use crate::{
    pages::settings::providers::Check,
    ui::{
        Type,
        widgets::{Focusable, card, labeled},
    },
    workspace::Workspace,
};
use dusk_protocol::config::{VoiceEndpoint, VoiceSettings};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
        input::Input,
        tooltip::Tooltip,
    },
    *,
};

const PROVIDER_TRACK: f32 = 220.;

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
    let form = &ws.voice_form;
    let current = ws.voice.as_ref();
    let panel = |title, endpoint, cx: &App| {
        card(cx)
            .flex_1()
            .flex_basis(px(PROVIDER_TRACK))
            .min_w_0()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(div().body().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(status(endpoint, cx)),
            )
    };
    let recognition = panel(
        "Speech recognition",
        current.and_then(|settings| settings.stt.as_ref()),
        cx,
    )
    .child(labeled("Address", "", cx).child(Input::new(&form.stt_url)))
    .child(labeled("Model", "", cx).child(Input::new(&form.stt_model)))
    .child(labeled("API key", "Empty keeps the saved key", cx).child(Input::new(&form.stt_key)));
    let synthesis = panel(
        "Speech synthesis",
        current.and_then(|settings| settings.tts.as_ref()),
        cx,
    )
    .child(labeled("Address", "", cx).child(Input::new(&form.tts_url)))
    .child(labeled("Model", "", cx).child(Input::new(&form.tts_model)))
    .child(labeled("API key", "Empty keeps the saved key", cx).child(Input::new(&form.tts_key)))
    .child(labeled("Voice", "Optional", cx).child(Input::new(&form.tts_voice)));
    let endpoints = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(super::super::section(
            "Speech providers",
            "Connect the microphone and read-aloud controls to an OpenAI-compatible audio server.",
            cx,
        ))
        .child(sources(cx))
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .child(recognition)
                .child(synthesis),
        )
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .flex_wrap()
                .gap_2()
                .child(
                    div()
                        .small()
                        .text_color(cx.theme().muted_foreground)
                        .child("API keys are stored in the system keychain."),
                )
                .child(
                    Button::new("voice-save")
                        .primary()
                        .label("Save voice settings")
                        .on_click(cx.listener(|ws, _, _, cx| ws.save_voice(cx))),
                ),
        );
    div()
        .flex()
        .flex_col()
        .gap_6()
        .child(endpoints)
        .child(super::models::render(ws, cx))
        .child(
            div().flex().flex_col().gap_3()
                .child(super::super::section(
                    "Other models",
                    "Run one behind a server that speaks the OpenAI audio API, then enter its address above. Set tokens for gated models in Settings, Secrets.",
                    cx,
                ))
                .child(other_models(cx)),
        )
}

/// The dot and the words that say whether a server is set up.
fn status(endpoint: Option<&VoiceEndpoint>, cx: &App) -> Div {
    match endpoint {
        Some(endpoint) => Check::Done(Ok(format!("{} at {}", endpoint.model, endpoint.base_url)))
            .line("Set up", cx),
        None => div()
            .small()
            .text_color(cx.theme().muted_foreground)
            .child("Not connected"),
    }
}

/// Where to start: a tile that fills the boxes with a known server.
fn sources(cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_wrap()
        .gap_3()
        .child(preset(
            "voice-preset-openai",
            "OpenAI",
            "api.openai.com",
            "https://api.openai.com/v1",
            ("whisper-1", "tts-1", "alloy"),
            cx,
        ))
        .child(preset(
            "voice-preset-local",
            "This machine",
            "a model on port 8000",
            "http://127.0.0.1:8000/v1",
            ("", "", ""),
            cx,
        ))
}

/// The models to try, as tiles that open their page on Hugging Face.
fn other_models(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().gap_3();
    for (title, items) in [("Recognition", RECOGNITION), ("Synthesis", SYNTHESIS)] {
        let mut row = div().flex().flex_wrap().gap_2();
        for (name, url) in items {
            row = row.child(
                tile(SharedString::from(format!("voice-model-{name}")), cx)
                    .child(div().body().font_weight(FontWeight::MEDIUM).child(name))
                    .child(div().meta().text_color(muted).child(title))
                    .tooltip(|window, cx| Tooltip::new("Open on Hugging Face").build(window, cx))
                    .on_click(cx.listener(move |_, _, _, cx| cx.open_url(url))),
            );
        }
        column = column.child(row);
    }
    column
}

/// A box that can be chosen, the way the provider kinds are.
fn tile(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    div()
        .id(id)
        .flex_1()
        .flex_basis(px(142.))
        .min_w_0()
        .flex()
        .flex_col()
        .gap_0p5()
        .px_3()
        .py_2()
        .rounded_lg()
        .cursor_pointer()
        .focusable_row(cx)
        .border_color(theme.border)
        .hover(move |tile| tile.bg(theme.accent))
        .press(theme.accent, cx)
}

/// A tile that fills the boxes with a known server's address and models.
fn preset(
    id: &'static str,
    label: &'static str,
    about: &'static str,
    url: &'static str,
    (stt, tts, voice): (&'static str, &'static str, &'static str),
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    tile(id, cx)
        .flex_basis(px(PROVIDER_TRACK))
        .px_4()
        .child(div().body().font_weight(FontWeight::MEDIUM).child(label))
        .child(div().meta().text_color(muted).child(about))
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
