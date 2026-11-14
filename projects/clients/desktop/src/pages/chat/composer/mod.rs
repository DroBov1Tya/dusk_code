//! The message box: Enter sends, Shift+Enter starts a new line, Stop ends the turn.
//! The parts around it live apart: what goes with the message (`attach`), the marks beside
//! the box (`marks`), the `@` names (`mention`) and the way to the core (`send`).

mod attach;
mod marks;
mod mention;
mod send;

pub use attach::paste_label;
pub use marks::{AGENTS_BUTTON, SIDE_MARK, agents_button, mic_button};

use crate::ui::Type;
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{Paste, Textarea},
    },
    prelude::FluentBuilder,
    *,
};

use crate::workspace::Workspace;
use attach::{PASTE_THRESHOLD, attach_menu, strip};
use mention::mention_menu;

pub const WIDTH: f32 = 780.;
/// The width of the columns beside the box.
const SIDE: f32 = crate::ui::size::BAR;
/// From the foot of the composer to the middle of the box's lower row of buttons: the
/// row under the box and the box's own padding.
const SIDE_LIFT: f32 = 40.;

/// A column beside the box, its controls centered and level with the box's buttons.
fn side() -> Div {
    div()
        .w(px(SIDE))
        .flex_none()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .pb(px(SIDE_LIFT))
}

pub fn render(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    // Subagents may go on after the chat's own turn is over; Stop ends them too.
    let working = ws
        .shown_chat()
        .is_some_and(|chat| chat.working || chat.agents.values().any(|agent| agent.working));
    let attach = ws.attach_open.then(|| attach_menu(ws.shown.is_some(), cx));
    let queued = super::queue::render(ws, cx);
    // Output speed while the agent writes, from the core's rate events.
    let rate = ws
        .shown
        .as_ref()
        .filter(|_| working)
        .and_then(|id| ws.rates.get(id))
        .filter(|rate| **rate > 0)
        .map(|rate| {
            div()
                .meta()
                .text_color(cx.theme().muted_foreground)
                .pb_1()
                .child(format!("{rate} tok/s"))
        });
    // Each place around the box takes its parts from the layout map.
    let layout = crate::ui::theme::prefs(cx).layout.clone();
    let place =
        |parts: &[crate::ui::layout::Part], window: &mut Window, cx: &mut Context<Workspace>| {
            super::parts::region(ws, parts, window, cx)
        };
    let side_left = place(&layout.side_left, window, cx);
    let side_right = place(&layout.side_right, window, cx);
    let box_left = place(&layout.box_left, window, cx);
    let box_right = place(&layout.box_right, window, cx);
    let under_left = place(&layout.under_left, window, cx);
    let under_right = place(&layout.under_right, window, cx);
    let accent = cx.theme().primary;
    let writing = gpui_kit::Focusable::focus_handle(ws.composer.read(cx), cx).is_focused(window);
    let mentions = mention_menu(ws, cx).or_else(|| super::commands::menu(ws, cx));
    let chips = strip(ws, cx);
    div()
        .relative()
        .px_4()
        .pt_2()
        .max_w(px(WIDTH + 2. * SIDE))
        .mx_auto()
        .w_full()
        .children(queued)
        .child(
            div()
                .flex()
                .flex_row()
                .items_end()
                .gap_2()
                // The history on the left and the context on the right stand level with
                // the box's lower row of buttons, as wide as each other.
                .child(side().children(side_left))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(
                            // One card: what goes with the message on top, the text under it
                            // across the whole width, and a row of controls at the bottom.
                            // The card grows upward, as the box sits at the foot of the chat.
                            div()
                                .flex()
                                .flex_col()
                                .rounded_2xl()
                                .border_1()
                                .border_color(if writing {
                                    accent.opacity(0.5)
                                } else {
                                    cx.theme().border
                                })
                                .bg(cx.theme().muted)
                                // The box lights up a little while it is written in.
                                .when(writing, |card| {
                                    card.shadow(vec![BoxShadow {
                                        color: accent.opacity(0.14),
                                        offset: point(px(0.), px(0.)),
                                        blur_radius: px(18.),
                                        spread_radius: px(0.),
                                        inset: false,
                                    }])
                                })
                                .shadow_sm()
                                .relative()
                                .children(mentions.map(crate::ui::widgets::on_top))
                                .children(chips)
                                .child(
                                    div()
                                        .w_full()
                                        .px_2()
                                        .pt_1()
                                        .child(Textarea::new(&ws.composer).appearance(false)),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap_1()
                                        .px_2()
                                        .pb_2()
                                        .children(box_left)
                                        .child(div().flex_1())
                                        .children(rate)
                                        .children(box_right),
                                ),
                        )
                        // Under the box: the mode on the left, the model on the right.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .pt_1()
                                .px_1()
                                .child(div().flex().items_center().children(under_left))
                                .child(div().flex().items_center().children(under_right)),
                        ),
                )
                // The subagents stand beside the box, not in it: they are not part of what
                // is sent.
                .child(side().children(side_right)),
        )
        .children(attach.map(crate::ui::widgets::on_top))
        // Files copied in the file manager arrive as files, not as their names.
        .capture_action(cx.listener(|ws, _: &Paste, _, cx| {
            let Some(item) = cx.read_from_clipboard() else {
                return;
            };
            let mut found = false;
            for entry in item.entries() {
                match entry {
                    ClipboardEntry::ExternalPaths(paths) => {
                        ws.attachments.extend(paths.paths().iter().cloned());
                        found = true;
                    }
                    // A screenshot becomes a file; on send the core copies it among the
                    // chat's sources, as it does a dropped picture.
                    ClipboardEntry::Image(image)
                        if matches!(image.format, ImageFormat::Png | ImageFormat::Jpeg) =>
                    {
                        match ws.keep_pasted_image(image) {
                            Ok(path) => ws.attachments.push(path),
                            Err(error) => ws.fail(error, cx),
                        }
                        found = true;
                    }
                    _ => {}
                }
            }
            // A long text becomes a chip; the core keeps it as a file of the chat.
            if !found
                && let Some(text) = item.text()
                && text.chars().count() > PASTE_THRESHOLD
            {
                ws.pastes.push(text);
                found = true;
            }
            if found {
                cx.stop_propagation();
                cx.notify();
            }
        }))
        // Tab switches the mode, before the text box takes it as an indent.
        .capture_key_down(cx.listener(|ws, event: &KeyDownEvent, window, cx| {
            let keys = &event.keystroke;
            if keys.key == "tab" && !keys.modifiers.shift && !keys.modifiers.control {
                cx.stop_propagation();
                // Over a mention or a command, Tab completes it, as in the TUI.
                if let Some(name) = ws.mention_matches(cx).first().cloned() {
                    ws.complete_mention(name, cx);
                } else if let Some((name, _)) = ws.command_matches(cx).first().copied() {
                    ws.complete_command(name, window, cx);
                } else {
                    ws.next_mode(cx);
                }
            }
        }))
}
