//! The parts around the message box, each drawn on its own, so the layout map
//! (`ui::layout`) can put any of them in any place around the box.

use gpui_kit::{
    component::{
        IconName, Sizable,
        button::{Button, ButtonRounded, ButtonVariants},
    },
    *,
};

use crate::{ui::layout::Part, workspace::Workspace};

/// The parts of `parts` this place holds, in order; the others are passed over.
pub fn region(
    ws: &Workspace,
    parts: &[Part],
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Vec<AnyElement> {
    parts
        .iter()
        .filter_map(|part| place(ws, *part, window, cx))
        .collect()
}

fn place(
    ws: &Workspace,
    part: Part,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Option<AnyElement> {
    Some(match part {
        Part::Attach => Button::new("attach")
            .ghost()
            .rounded(ButtonRounded::Size(px(999.)))
            .icon(IconName::Plus)
            .tooltip("Attach files or folders")
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.attach_open = !ws.attach_open;
                cx.notify();
            }))
            .into_any_element(),
        Part::Access => super::model_bar::access(ws, cx)?.into_any_element(),
        Part::Microphone => super::composer::mic_button(ws, cx).into_any_element(),
        Part::Send => send(ws, cx),
        Part::Mode => super::model_bar::mode(ws, cx).into_any_element(),
        Part::Model => super::model_bar::models(ws, window, cx).into_any_element(),
        Part::History => (ws.sent_rows.len() > 1)
            .then(|| super::navigator::render(ws, &ws.sent_rows, window, cx))?
            .into_any_element(),
        Part::Agents => super::composer::agents_button(ws, cx)?.into_any_element(),
        Part::Plugins => crate::plugins::place::composer(ws, cx)?,
        // The account's limits sit over the context, when the provider reports them.
        Part::Context => div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .children(super::model_bar::limits(ws, cx))
            .child(super::model_bar::context(ws, cx))
            .into_any_element(),
        _ => return None,
    })
}

/// Send, or Stop while the chat or one of its subagents works. Text typed meanwhile can
/// go at once, as Enter sends it: a quiet Stop then stands beside Send.
fn send(ws: &Workspace, cx: &mut Context<Workspace>) -> AnyElement {
    let working = ws
        .shown_chat()
        .is_some_and(|chat| chat.working || chat.agents.values().any(|agent| agent.working));
    let send = Button::new("send")
        .primary()
        .rounded(ButtonRounded::Size(px(999.)))
        .icon(IconName::ArrowUp)
        .tooltip("Send")
        .on_click(cx.listener(|ws, _, window, cx| ws.send(window, cx)));
    if !working {
        return send.into_any_element();
    }
    let stop = Button::new("stop")
        .rounded(ButtonRounded::Size(px(999.)))
        .icon(IconName::Square)
        .tooltip("Stop")
        .on_click(cx.listener(|ws, _, _, cx| ws.cancel(cx)));
    let typed = !ws.composer.read(cx).value().trim().is_empty()
        || !ws.attachments.is_empty()
        || !ws.pastes.is_empty();
    if !typed {
        return stop.danger().into_any_element();
    }
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .child(stop.ghost().small())
        .child(send)
        .into_any_element()
}
