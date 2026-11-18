//! The welcome drawn: the greeting with its one large button, then the two boxes.

use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Sizable,
        button::{Button, ButtonVariants},
        input::Input,
    },
    *,
};

use super::Step;
use crate::{
    platform::icon,
    ui::{
        Type,
        widgets::{appear, labeled},
    },
    workspace::Workspace,
};

/// Wide enough for an address, narrow enough to read as one thing.
const WIDTH: f32 = 420.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let body = match ws.welcome.step {
        Step::Form => appear("welcome-form", form(ws, cx)).into_any_element(),
        _ => appear("welcome-intro", intro(cx)).into_any_element(),
    };
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .px_6()
        .child(body)
}

fn column() -> Div {
    div().w_full().max_w(px(WIDTH)).flex().flex_col()
}

fn intro(cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    let (primary, hover, on_primary) =
        (theme.primary, theme.primary_hover, theme.primary_foreground);
    let button = div()
        .id("welcome-connect")
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .px_6()
        .py_4()
        .rounded_xl()
        .bg(primary)
        .hover(|button| button.bg(hover))
        .cursor_pointer()
        .text_color(on_primary)
        .child(
            div()
                .text_size(rems(1.15))
                .font_weight(FontWeight::BOLD)
                .child("I want to connect my own AI"),
        )
        .child(div().small().opacity(0.8).child("your own provider"))
        .on_click(cx.listener(|ws, _, window, cx| ws.welcome_form(window, cx)));
    let not_now = div()
        .id("welcome-not-now")
        .small()
        .text_color(muted)
        .hover(|link| link.text_color(foreground))
        .cursor_pointer()
        .child("Not now")
        .on_click(cx.listener(|ws, _, _, cx| ws.welcome_dismiss(cx)));
    column()
        .items_center()
        .gap_6()
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .gap_3()
                .child(img(icon::mark()).size(px(56.)).flex_none())
                .child(div().display().child("duskcode"))
                .child(
                    div()
                        .body()
                        .text_center()
                        .text_color(muted)
                        .child("An agent that works in your projects, on the model you choose."),
                ),
        )
        .child(button)
        .child(not_now)
}

fn form(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let welcome = &ws.welcome;
    let theme = cx.theme();
    let (muted, foreground, danger) = (theme.muted_foreground, theme.foreground, theme.danger);
    let back = div()
        .id("welcome-back")
        .small()
        .text_color(muted)
        .hover(|link| link.text_color(foreground))
        .cursor_pointer()
        .child("Back")
        .on_click(cx.listener(|ws, _, _, cx| {
            ws.welcome_back(cx);
        }));
    column()
        .gap_4()
        .child(div().title().child("Your own provider"))
        .child(
            labeled("Address", "a server with the OpenAI API", cx)
                .child(Input::new(&welcome.address).large()),
        )
        .child(
            labeled("Token", "kept in the system keychain", cx)
                .child(Input::new(&welcome.token).large()),
        )
        .children(
            welcome
                .error
                .clone()
                .map(|error| div().small().text_color(danger).child(error)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pt_1()
                .child(back)
                .child(
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
