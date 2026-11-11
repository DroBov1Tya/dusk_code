//! The Voice shelf: the two speech servers, one that turns the microphone into text and one
//! that reads answers aloud. Any server that speaks the OpenAI audio API will do: a hosted
//! account, or a model run on this machine. Keys go to the keychain, never the config file.

use dusk_protocol::voice::{STT_KEY, TTS_KEY, VoiceEndpoint, VoiceSettings};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use crate::{ui::Type, widgets, workspace::Workspace};

/// The boxes of the two forms.
pub struct VoiceForm {
    stt_url: Entity<InputState>,
    stt_model: Entity<InputState>,
    stt_key: Entity<InputState>,
    tts_url: Entity<InputState>,
    tts_model: Entity<InputState>,
    tts_voice: Entity<InputState>,
    tts_key: Entity<InputState>,
    /// Settings that arrived and are to be put in the boxes when next drawn.
    fill: Option<VoiceSettings>,
    /// The key boxes are emptied when next drawn.
    clear_keys: bool,
}

impl VoiceForm {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let field = |hint: &'static str, masked: bool, window: &mut Window, cx: &mut App| {
            cx.new(|cx| InputState::new(window, cx).placeholder(hint).masked(masked))
        };
        Self {
            stt_url: field("https://host/v1", false, window, cx),
            stt_model: field("Model, such as whisper-1", false, window, cx),
            stt_key: field(
                "API key (leave empty to keep, or for a local server)",
                true,
                window,
                cx,
            ),
            tts_url: field("https://host/v1", false, window, cx),
            tts_model: field("Model, such as tts-1", false, window, cx),
            tts_voice: field("Voice, such as alloy (optional)", false, window, cx),
            tts_key: field(
                "API key (leave empty to keep, or for a local server)",
                true,
                window,
                cx,
            ),
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

/// What runs once the server a button needs is connected.
pub type Ready = Box<dyn FnOnce(&mut Workspace, &mut Context<Workspace>)>;

/// Which of the two speech servers a button needs.
#[derive(Clone, Copy)]
pub enum Speech {
    Recognition,
    Synthesis,
}

/// The window that says a button's server is not connected, and offers to connect it.
pub fn missing(speech: Speech, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (title, text) = match speech {
        Speech::Recognition => (
            "Speech recognition is not connected",
            "The microphone needs a server that turns speech into text: a hosted account, or a \
             model run on this machine.",
        ),
        Speech::Synthesis => (
            "Speech synthesis is not connected",
            "Reading aloud needs a server that turns text into speech: a hosted account, or a \
             model run on this machine.",
        ),
    };
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
        .child(div().text_sm().text_color(muted).child(text))
        .child(
            div()
                .flex()
                .flex_row()
                .justify_end()
                .gap_2()
                .child(
                    Button::new("voice-missing-cancel")
                        .ghost()
                        .label("Not now")
                        .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
                )
                .child(
                    Button::new("voice-missing-connect")
                        .primary()
                        .label("Connect")
                        .on_click(cx.listener(|ws, _, _, cx| ws.open_voice_setup(cx))),
                ),
        )
}

impl Workspace {
    /// Runs `ready` when the server `speech` names is connected; otherwise asks the user to
    /// connect it. The core is asked each time, so a server connected a moment ago counts.
    pub fn when_voice_ready(&mut self, speech: Speech, cx: &mut Context<Self>, ready: Ready) {
        self.core.call(
            cx,
            |client| async move { client.catalog().voice().await },
            move |ws, result, cx| match result {
                Ok(voice) => {
                    let connected = match speech {
                        Speech::Recognition => voice.stt.is_some(),
                        Speech::Synthesis => voice.tts.is_some(),
                    };
                    ws.voice = Some(voice);
                    if connected {
                        ready(ws, cx);
                    } else {
                        ws.modal = Some(crate::modal::Modal::VoiceMissing { speech });
                        cx.notify();
                    }
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Opens Connectors on the Voice shelf.
    fn open_voice_setup(&mut self, cx: &mut Context<Self>) {
        self.modal = None;
        self.center = crate::workspace::Center::Connectors;
        self.shelf = super::Shelf::Voice;
        self.load_settings(cx);
        self.load_voice(cx);
    }

    pub fn load_voice(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.catalog().voice().await },
            |ws, result, cx| match result {
                Ok(voice) => {
                    ws.voice_form.fill = Some(voice.clone());
                    ws.voice = Some(voice);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn save_voice(&mut self, cx: &mut Context<Self>) {
        let form = &self.voice_form;
        let text = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        let endpoint = |url: String, model: String, voice: String| {
            (!url.is_empty()).then(|| VoiceEndpoint {
                base_url: url,
                model,
                voice: (!voice.is_empty()).then_some(voice),
            })
        };
        let settings = VoiceSettings {
            stt: endpoint(text(&form.stt_url), text(&form.stt_model), String::new()),
            tts: endpoint(
                text(&form.tts_url),
                text(&form.tts_model),
                text(&form.tts_voice),
            ),
        };
        let (stt_key, tts_key) = (text(&form.stt_key), text(&form.tts_key));
        self.core.call(
            cx,
            move |client| async move {
                let settings_client = client.settings();
                // A key typed now is kept first, so a server never starts without it.
                if !stt_key.is_empty() {
                    settings_client.set_secret(STT_KEY, &stt_key).await?;
                }
                if !tts_key.is_empty() {
                    settings_client.set_secret(TTS_KEY, &tts_key).await?;
                }
                settings_client.set_voice(settings).await
            },
            |ws, result, cx| match result {
                Ok(()) => {
                    ws.voice_form.clear_keys = true;
                    ws.load_voice(cx);
                    ws.load_secrets(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
