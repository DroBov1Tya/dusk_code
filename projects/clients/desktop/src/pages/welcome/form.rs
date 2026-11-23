//! The Model page: a name, an address and a token, as before; once a provider is
//! connected, here or anywhere else, the page says which, and Next takes its place.

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    *,
};

use dusk_protocol::model::CheckFailure;

use crate::{
    ui::{Type, widgets::labeled},
    workspace::Workspace,
};

/// The provider the welcome can name as connected: the one connected here, else any
/// the core has.
pub fn connected(ws: &Workspace) -> Option<String> {
    let joined = ws.welcome.joined.as_ref().map(|(name, _)| name.clone());
    joined.or_else(|| ws.providers.first().map(|provider| provider.name.clone()))
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let heading = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().title().child("Connect a model"))
        .child(
            div()
                .body()
                .text_color(cx.theme().muted_foreground)
                .child("Any server with the OpenAI API: a cloud provider or one on this machine."),
        );
    let page = div().flex().flex_col().gap_5().child(heading);
    match connected(ws) {
        Some(name) => page.child(joined(ws, name, cx)),
        None => page.child(form(ws, cx)),
    }
}

/// The provider connected, and the model new chats start with when it is known.
fn joined(ws: &Workspace, name: String, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (success, muted) = (theme.success, theme.muted_foreground);
    let model = ws
        .welcome
        .joined
        .as_ref()
        .and_then(|(_, model)| model.clone());
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .py_4()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .child(
            Icon::new(IconName::CircleCheck)
                .text_color(success)
                .flex_none(),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .min_w_0()
                .child(div().body().child(format!("Connected to {name}")))
                .children(model.map(|model| {
                    div()
                        .small()
                        .text_color(muted)
                        .truncate()
                        .child(format!("New chats use {model}"))
                })),
        )
}

fn form(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let welcome = &ws.welcome;
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(
            labeled("Name", "how the model menu will call it", cx)
                .child(Input::new(&welcome.name).large()),
        )
        .child(
            labeled("Address", "a server with the OpenAI API", cx)
                .child(Input::new(&welcome.address).large()),
        )
        .child(
            labeled("Token", "kept in the system keychain", cx)
                .child(Input::new(&welcome.token).large()),
        )
        .children(welcome.error.as_ref().map(|failed| failure(failed, cx)))
        .child(
            div().flex().flex_row().justify_end().child(
                Button::new("welcome-submit")
                    .primary()
                    .large()
                    .label(if welcome.busy {
                        "Connecting..."
                    } else {
                        "Connect"
                    })
                    .disabled(welcome.busy)
                    .on_click(cx.listener(|ws, _, window, cx| ws.welcome_connect(window, cx))),
            ),
        )
}

/// One plain line for the kind of failure, then the core's own words, quieter; without
/// a kind, the core's words alone.
fn failure(failed: &super::Failed, cx: &App) -> Div {
    let theme = cx.theme();
    let headline = match failed.cause {
        Some(CheckFailure::Unreachable) => Some("Cannot reach this address."),
        Some(CheckFailure::Refused) => Some("The server refused the token."),
        Some(CheckFailure::NotCompatible) => {
            Some("The server did not answer like an OpenAI-compatible API.")
        }
        None => None,
    };
    let said = div().child(failed.said.clone());
    let column = div().flex().flex_col().gap_1();
    match headline {
        Some(headline) => column
            .child(div().small().text_color(theme.danger).child(headline))
            .child(said.meta().text_color(theme.muted_foreground)),
        None => column.child(said.small().text_color(theme.danger)),
    }
}
