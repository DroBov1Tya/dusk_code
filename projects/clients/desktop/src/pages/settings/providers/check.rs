//! What a check of a provider found, as a line to show.

use dusk_protocol::model::ProviderStatus;
use gpui_kit::{component::ActiveTheme, *};

#[derive(Clone)]
pub enum Check {
    Running,
    /// The provider answered, with this to say; or it did not, with this reason.
    Done(Result<String, String>),
}

impl Check {
    pub fn of(status: &ProviderStatus) -> Self {
        Self::Done(match &status.models {
            Ok(models) => {
                let count = crate::ui::widgets::count(models.len(), "model");
                Ok(match &status.balance {
                    Some(Ok(balance)) => format!("{count} \u{00B7} {}", balance.text()),
                    _ => count,
                })
            }
            Err(error) => Err(error.clone()),
        })
    }

    pub fn failed(error: &anyhow::Error) -> Self {
        Self::Done(Err(format!("{error:#}")))
    }

    /// The dot and the words of the result, in the color that says how it went.
    pub fn line(&self, connected_word: &str, cx: &App) -> Div {
        let theme = cx.theme();
        let (color, text) = match self {
            Self::Running => (theme.warning, "Checking...".to_owned()),
            Self::Done(Ok(found)) => (theme.success, format!("{connected_word}: {found}")),
            Self::Done(Err(error)) => (theme.danger, error.clone()),
        };
        div()
            .flex()
            .flex_row()
            .items_start()
            .gap_2()
            .text_sm()
            .child(
                div()
                    .mt(px(6.))
                    .size(px(8.))
                    .flex_none()
                    .rounded_full()
                    .bg(color),
            )
            .child(div().min_w_0().text_color(color).child(text))
    }
}
