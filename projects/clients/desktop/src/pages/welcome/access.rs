//! The Access page: one row per approval mode, what the agent does in it without asking
//! and what it still asks for. A pick is saved at once as the default for new chats, the
//! setting Safety shows; nothing is picked for the user, so Next alone keeps the default.

use dusk_protocol::{client::Switch as CoreSwitch, config::ApprovalPolicy};
use gpui_kit::{component::ActiveTheme, prelude::FluentBuilder, *};

use crate::{
    pages::settings::list,
    ui::{
        Type,
        controls::{Mark, mark},
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// The modes from the least the agent does alone to the most.
const MODES: [(ApprovalPolicy, &str, &str); 4] = [
    (
        ApprovalPolicy::Ask,
        "Ask",
        "Edits files in your project on its own. Asks before running commands and before \
         anything outside the project.",
    ),
    (
        ApprovalPolicy::Sandbox,
        "Sandbox",
        "Runs commands without asking, locked so they can write only in your project. Asks \
         before anything outside it.",
    ),
    (
        ApprovalPolicy::Arbiter,
        "Auto",
        "A second model allows or refuses each request for you and says why. When it \
         cannot decide, it asks you.",
    ),
    (
        ApprovalPolicy::FullAccess,
        "Full access",
        "Runs commands and edits files without asking; only settings changes and steps on \
         your own branches still ask. A wrong command runs before you can stop it, so use it \
         where the work can be undone.",
    ),
];

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let heading = div()
        .flex()
        .flex_col()
        .gap_1()
        .child(div().title().child("How much may the agent do on its own?"))
        .child(
            div()
                .body()
                .text_color(muted)
                .child("This is the default for new chats."),
        );
    // Sandbox needs the system's confinement; unknown until the settings come.
    let sandbox = ws.settings.as_ref().is_none_or(|settings| settings.sandbox);
    let chosen = ws.status.approvals;
    let rows = MODES.map(|(policy, name, about)| {
        let usable = policy != ApprovalPolicy::Sandbox || sandbox;
        let about = if usable {
            about
        } else {
            "Not available here: this system cannot lock commands to the project."
        };
        row(policy, name, about, chosen == policy, usable, cx)
    });
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(heading)
        .child(list(rows, cx))
        .child(div().small().text_color(muted).child(
            "Change it any time in Settings, under Safety, or for one chat with the \
             access button in the message box.",
        ))
}

/// A mode to pick: the ring, its name (with the recommendation on Full access) and what
/// it lets the agent do.
fn row(
    policy: ApprovalPolicy,
    name: &'static str,
    about: &'static str,
    on: bool,
    usable: bool,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (muted, hover, accent) = (theme.muted_foreground, theme.list_hover, theme.primary);
    let title = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_x_2()
        .gap_y_1()
        .child(div().body().child(name))
        .when(policy == ApprovalPolicy::FullAccess, |title| {
            title.child(widgets::badge(
                "Recommended if you don't want to be asked",
                accent,
            ))
        });
    let text = div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .child(title)
        .child(div().small().text_color(muted).child(about));
    let row = div()
        .id(SharedString::from(format!(
            "welcome-access-{}",
            policy.name()
        )))
        .flex()
        .flex_row()
        .items_start()
        .gap_3()
        .px_4()
        .py_3()
        .child(div().pt_0p5().child(mark(Mark::Radio, on, cx)))
        .child(text);
    let row = if usable {
        row.cursor_pointer()
            .hover(move |row| row.bg(hover))
            .focusable_row(cx)
            .when(!on, |row| {
                row.on_click(cx.listener(move |ws, _, _, cx| {
                    ws.switch(CoreSwitch::Approvals(policy), true, cx);
                }))
            })
    } else {
        row.opacity(0.5)
    };
    div().child(row)
}
