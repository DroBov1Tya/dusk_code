//! Saving the speech servers, and asking to connect one when a button needs it.

use crate::workspace::Workspace;
use dusk_protocol::config::{STT_KEY, TTS_KEY, VoiceEndpoint, VoiceSettings};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
        input::InputState,
    },
    *,
};

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
        self.shelf = crate::pages::connectors::Shelf::Voice;
        self.load_settings(cx);
        self.load_voice(cx);
        self.load_voice_models(cx);
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

    pub(super) fn save_voice(&mut self, cx: &mut Context<Self>) {
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
