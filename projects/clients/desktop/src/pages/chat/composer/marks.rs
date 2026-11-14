//! The marks beside the box: the microphone and the chat's subagents.

use crate::ui::{Type, widgets::Focusable};
use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonRounded, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::workspace::Workspace;

/// The marks beside the box, the history on the left and the subagents and the context on
/// the right: one size and one shape, a circle, so the two sides mirror each other.
pub const SIDE_MARK: f32 = crate::ui::size::CONTROL + 4.;
pub const AGENTS_BUTTON: f32 = SIDE_MARK;

/// The microphone: while it is on, what is said is added to the message, and rings go
/// out from it like sound, so a recording under way is never mistaken for a red button.
pub fn mic_button(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let on = ws.dictating();
    let danger = cx.theme().danger;
    let button = Button::new("dictate")
        .rounded(ButtonRounded::Size(px(999.)))
        .icon(IconName::Mic)
        // The size of Send beside it; filled only while it listens.
        .when(on, |button| button.danger())
        .when(!on, |button| button.ghost())
        .tooltip(if on { "Stop dictating" } else { "Dictate" })
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_dictation(cx)));
    let mut holder = div().relative().flex().items_center().justify_center();
    if on {
        // Two rings half a beat apart; each grows from the button's edge and thins out.
        for (name, shift) in [("dictate-ring-a", 0.), ("dictate-ring-b", 0.5)] {
            holder = holder.child(
                div()
                    .absolute()
                    .rounded_full()
                    .border_1()
                    .border_color(danger)
                    .with_animation(
                        name,
                        Animation::new(std::time::Duration::from_millis(1600)).repeat(),
                        move |ring, at| {
                            let at = (at + shift).fract();
                            let grown = 32. + 22. * at;
                            ring.size(px(grown)).opacity(0.55 * (1. - at) * (1. - at))
                        },
                    ),
            );
        }
    }
    holder.child(button)
}

/// The chat's subagents at a glance, beside the box: the figure of the one that works (or
/// the latest), a count, and a ring in the signal color while any works. A click opens
/// the stage where each is seen at work.
pub fn agents_button(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = ws.shown_chat().filter(|chat| !chat.agents.is_empty())?;
    let thread_id = ws.shown.clone()?;
    let working = chat.agents.values().filter(|agent| agent.working).count();
    let lead = chat
        .agents
        .iter()
        .filter(|(_, agent)| agent.working)
        .min_by_key(|(_, agent)| agent.started)
        .or_else(|| chat.agents.iter().max_by_key(|(_, agent)| agent.started))?;
    let theme = cx.theme();
    let (signal, muted, border, surface) = (
        theme.warning,
        theme.muted_foreground,
        theme.border,
        theme.muted,
    );
    let color = if working > 0 { signal } else { muted };
    let hover = theme.list_hover;
    let total = chat.agents.len();
    let open = matches!(&ws.modal, Some(crate::modal::Modal::Agents { thread_id: open }) if open == &thread_id);
    let label = format!("Subagents, {working} active of {total}");
    let tile = div()
        .id("agents")
        .focusable_row(cx)
        .track_focus(&ws.agents_trigger_focus)
        .role(Role::Button)
        .aria_label(label.clone())
        .aria_expanded(open)
        .size(px(AGENTS_BUTTON))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(if working > 0 {
            signal.opacity(0.7)
        } else {
            border
        })
        .bg(surface)
        .cursor_pointer()
        .hover(move |button| button.bg(hover).border_color(muted.opacity(0.6)))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(label.clone()).build(window, cx)
        })
        .child(crate::columns::inspector::mascot::render(
            "agents-lead",
            &lead.1.state,
            color,
            1.0,
        ))
        .on_click(cx.listener(move |ws, _, window, cx| {
            ws.modal = Some(crate::modal::Modal::Agents {
                thread_id: thread_id.clone(),
            });
            ws.agents_modal_focus.focus(window, cx);
            cx.notify();
        }));
    // The count sits on the circle's upper right, half outside it.
    const BADGE_ROOM: f32 = 6.;
    Some(
        div()
            .relative()
            .flex_none()
            .pt(px(BADGE_ROOM))
            .pr(px(BADGE_ROOM))
            // The room is outside the layout: the tile stays where it was, at its size.
            .mt(px(-BADGE_ROOM))
            .mr(px(-BADGE_ROOM))
            .child(tile)
            .child(
                div()
                    .absolute()
                    .top_0()
                    .right_0()
                    .min_w(px(16.))
                    .h(px(16.))
                    .px_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .meta()
                    .font_weight(FontWeight::MEDIUM)
                    .bg(if working > 0 { signal } else { surface })
                    .text_color(if working > 0 { theme.background } else { muted })
                    .border_1()
                    .border_color(border)
                    .child(chat.agents.len().to_string()),
            ),
    )
}
