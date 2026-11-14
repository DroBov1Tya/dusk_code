//! The speech shelf: the two server cards, where to start, and the models to try.

use crate::{
    pages::settings::providers::Check,
    ui::{
        Type,
        widgets::{card, labeled},
    },
    workspace::Workspace,
};
use dusk_protocol::config::{VoiceEndpoint, VoiceSettings};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
        tooltip::Tooltip,
    },
    *,
};

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
    let current = ws.voice.clone().unwrap_or_default();
    let mut page = div().flex().flex_col().gap_4().max_w(px(760.));
    page = page
        .child(div().small().text_color(muted).child(
            "Speech goes through any server that speaks the OpenAI audio API: a hosted account, \
             or a model run on this machine. Keys are kept in the system keychain.",
        ))
        .child(super::models::render(ws, cx))
        .child(heading(
            "Or connect a server",
            "A hosted account, or any server of yours that speaks the OpenAI audio API.",
            cx,
        ))
        .child(sources(cx))
        .child(
            card(cx)
                .child(heading(
                    "Speech recognition",
                    "the microphone: your voice becomes text",
                    cx,
                ))
                .child(status(current.stt.as_ref(), cx))
                .child(
                    labeled("Address", "where the server is", cx)
                        .child(Input::new(&form.stt_url).large()),
                )
                .child(
                    labeled("Model", "such as whisper-1", cx)
                        .child(Input::new(&form.stt_model).large()),
                )
                .child(
                    labeled(
                        "API key",
                        "empty keeps the saved one, or for a local server",
                        cx,
                    )
                    .child(Input::new(&form.stt_key).large()),
                ),
        )
        .child(
            card(cx)
                .child(heading(
                    "Speech synthesis",
                    "read aloud: text becomes sound",
                    cx,
                ))
                .child(status(current.tts.as_ref(), cx))
                .child(
                    labeled("Address", "where the server is", cx)
                        .child(Input::new(&form.tts_url).large()),
                )
                .child(
                    labeled("Model", "such as tts-1", cx)
                        .child(Input::new(&form.tts_model).large()),
                )
                .child(
                    labeled("Voice", "optional, such as alloy", cx)
                        .child(Input::new(&form.tts_voice).large()),
                )
                .child(
                    labeled(
                        "API key",
                        "empty keeps the saved one, or for a local server",
                        cx,
                    )
                    .child(Input::new(&form.tts_key).large()),
                ),
        )
        .child(
            div().flex().flex_row().child(
                Button::new("voice-save")
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(|ws, _, _, cx| ws.save_voice(cx))),
            ),
        );
    page = page
        .child(heading(
            "Other models",
            "Run one behind a server that speaks the OpenAI audio API, then point the boxes \
             above at it. A token for gated models is set in Settings, Secrets.",
            cx,
        ))
        .child(other_models(cx));
    page
}

/// A card's title and what it is for.
fn heading(title: &'static str, about: &'static str, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(div().body().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child(about),
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
            .child("Not set up"),
    }
}

/// Where to start: a tile that fills the boxes with a known server.
fn sources(cx: &mut Context<Workspace>) -> Div {
    div()
        .flex()
        .flex_row()
        .gap_2()
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
        let mut row = div().flex().flex_row().gap_2();
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
        .flex()
        .flex_col()
        .gap_0p5()
        .px_3()
        .py_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.border)
        .cursor_pointer()
        .hover(move |tile| tile.bg(theme.accent))
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
