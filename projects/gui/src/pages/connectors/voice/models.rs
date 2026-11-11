//! The models that run on this machine: what each is, where it stands, and the buttons that
//! install it, use it or take it away. The core does the work; this page asks and shows.

use std::time::Duration;

use dusk_protocol::config::{LocalState, SpeechKind, VoiceModel, VoiceModels};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::settings::providers::Check,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// How often the core is asked while something installs.
const POLL: Duration = Duration::from_millis(1500);

const UV_PAGE: &str = "https://docs.astral.sh/uv/getting-started/installation/";

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().gap_3();
    column = column.child(
        div()
            .flex()
            .flex_col()
            .gap_0p5()
            .child(
                div()
                    .body()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Run on this machine"),
            )
            .child(div().small().text_color(muted).child(
                "Installed by the app into its own folder and started with it. The model \
                 and your voice stay on this machine.",
            )),
    );
    let Some(listed) = &ws.voice_models else {
        return column.child(widgets::muted("Looking...", cx).px_0());
    };
    if !listed.uv_found {
        column = column.child(need_uv(cx));
    }
    for model in &listed.models {
        column = column.child(model_card(model, listed.uv_found, cx));
    }
    column
}

/// Says that `uv` is needed, and that everything after it is automatic.
fn need_uv(cx: &mut Context<Workspace>) -> Div {
    let (warning, muted) = (cx.theme().warning, cx.theme().muted_foreground);
    widgets::card(cx)
        .border_color(warning.opacity(0.5))
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child("uv is not installed"),
        )
        .child(div().small().text_color(muted).child(
            "The models are set up with uv, a small tool that fetches Python and the packages. \
             Install it once; after that each model installs by itself with one click: the \
             environment, the packages and the weights.",
        ))
        .child(
            div().flex().flex_row().child(
                Button::new("voice-uv-page")
                    .outline()
                    .label("How to install uv")
                    .on_click(cx.listener(|_, _, _, cx| cx.open_url(UV_PAGE))),
            ),
        )
}

fn model_card(model: &VoiceModel, uv_found: bool, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let way = match model.kind {
        SpeechKind::Recognition => "recognition",
        SpeechKind::Synthesis => "synthesis",
    };
    let id = model.id.clone();
    let installing = matches!(model.state, LocalState::Installing(_));
    let present = matches!(model.state, LocalState::Installed | LocalState::Running);
    let button = |label: &'static str, suffix: &str| {
        Button::new(SharedString::from(format!("voice-{suffix}-{}", model.id)))
            .small()
            .label(label)
    };
    let mut actions = div().flex().flex_row().items_center().gap_2();
    match &model.state {
        LocalState::NotInstalled | LocalState::Failed(_) => {
            let label = if matches!(model.state, LocalState::Failed(_)) {
                "Try again"
            } else {
                "Install"
            };
            let id = id.clone();
            actions = actions.child(
                button(label, "install")
                    .primary()
                    .disabled(!uv_found)
                    .tooltip(format!("Downloads {}", model.size))
                    .on_click(cx.listener(move |ws, _, _, cx| ws.install_voice_model(&id, cx))),
            );
        }
        LocalState::Installed | LocalState::Running if model.in_use => {
            actions = actions.child(div().small().text_color(muted).child("In use"));
        }
        LocalState::Installed | LocalState::Running => {
            let id = id.clone();
            actions = actions.child(
                button("Use", "use")
                    .primary()
                    .on_click(cx.listener(move |ws, _, _, cx| ws.install_voice_model(&id, cx))),
            );
        }
        LocalState::Installing(_) => {}
    }
    let remove = id.clone();
    actions = actions.when(
        present || matches!(model.state, LocalState::Failed(_)),
        |row| {
            row.child(
                button("Remove", "remove")
                    .ghost()
                    .tooltip("Delete the model and what it downloaded")
                    .on_click(cx.listener(move |ws, _, _, cx| ws.remove_voice_model(&remove, cx))),
            )
        },
    );
    widgets::card(cx)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_row()
                        .items_baseline()
                        .gap_2()
                        .child(
                            div()
                                .body()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(model.title.clone()),
                        )
                        .child(div().meta().text_color(muted).child(way)),
                )
                .child(actions),
        )
        .child(div().small().text_color(muted).child(format!(
            "{} \u{00B7} {} \u{00B7} {}",
            model.about, model.size, model.languages
        )))
        .children(state_line(&model.state, installing, cx))
}

fn state_line(state: &LocalState, installing: bool, cx: &App) -> Option<Div> {
    match state {
        LocalState::NotInstalled => None,
        LocalState::Installing(stage) => Some(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .child(crate::ui::spinner(cx))
                .child(
                    div()
                        .small()
                        .text_color(cx.theme().warning)
                        .child(format!("{stage}...")),
                ),
        ),
        LocalState::Installed => Some(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child("Installed, not running"),
        ),
        LocalState::Running => Some(Check::Done(Ok("running".into())).line("It is", cx)),
        LocalState::Failed(why) => {
            debug_assert!(!installing);
            Some(Check::Done(Err(why.clone())).line("", cx))
        }
    }
}

impl Workspace {
    /// Asks the core where the models are, and again while one installs.
    pub fn load_voice_models(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.voice_models().await },
            |ws, result, cx| match result {
                Ok(listed) => ws.take_voice_models(listed, cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn take_voice_models(&mut self, listed: VoiceModels, cx: &mut Context<Self>) {
        let busy = |models: &VoiceModels| {
            models
                .models
                .iter()
                .any(|model| matches!(model.state, LocalState::Installing(_)))
        };
        let was_busy = self.voice_models.as_ref().is_some_and(busy);
        let now_busy = busy(&listed);
        self.voice_models = Some(listed);
        if was_busy && !now_busy {
            // An install ended: the core pointed the speech at it, so the boxes follow.
            self.load_voice(cx);
        }
        if now_busy && !self.voice_polling {
            self.voice_polling = true;
            cx.spawn(async move |ws, cx| {
                cx.background_executor().timer(POLL).await;
                let _ = ws.update(cx, |ws, cx| {
                    ws.voice_polling = false;
                    ws.load_voice_models(cx);
                });
            })
            .detach();
        }
        cx.notify();
    }

    fn install_voice_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.install_voice_model(&id).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
                ws.load_voice_models(cx);
            },
        );
    }

    fn remove_voice_model(&mut self, id: &str, cx: &mut Context<Self>) {
        let id = id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.remove_voice_model(&id).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
                ws.load_voice_models(cx);
                ws.load_voice(cx);
            },
        );
    }
}
