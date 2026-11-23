//! The message box: Enter sends, Shift+Enter starts a new line, Stop ends the turn.
//! The parts around it live apart: what goes with the message (`attach`), the marks beside
//! the box (`marks`), the `@` names (`mention`) and the way to the core (`send`).

mod attach;
mod drafts;
mod marks;
mod mention;
mod send;
mod suggest;

pub use attach::paste_label;
pub use drafts::{Change, Draft, Drafts, Edit};
pub use marks::{AGENTS_BUTTON, SIDE_MARK, agents_button, mic_button};
pub use suggest::{PLACEHOLDER, show_suggestion};

use crate::ui::Type;
use crate::ui::elevation::{Elevation, Level, shadows};
use gpui_kit::{
    component::{
        ActiveTheme,
        input::{Paste, Textarea},
    },
    prelude::FluentBuilder,
    *,
};

use super::dock::panel;
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

/// The lane the dock's cards stand in: the box's own edges at every window width, so the
/// plan strip, a request for approval and a question line up with the box under them.
pub fn dock_lane(content: impl IntoElement) -> Div {
    div()
        .px_4()
        .max_w(px(WIDTH + 2. * SIDE))
        .mx_auto()
        .w_full()
        // A side column and the gap between it and the box.
        .child(div().px(px(SIDE + crate::ui::size::GAP_S)).child(content))
}

/// Puts the shown chat's draft in the composer when another chat's is there. Called as the
/// chat page is drawn: every way to another chat ends here, and here is the window.
pub fn show_draft(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let mut current = Draft {
        text: ws.composer.read(cx).value().to_string(),
        attachments: std::mem::take(&mut ws.attachments),
        pastes: std::mem::take(&mut ws.pastes),
    };
    let edited = !ws.fill_composer.is_empty();
    for edit in std::mem::take(&mut ws.fill_composer) {
        ws.drafts.edit(&mut current, edit);
    }
    let switched = ws
        .drafts
        .switch(ws.shown.as_deref(), || std::mem::take(&mut current));
    let switched_chat = switched.is_some();
    let draft = switched.unwrap_or(current);
    ws.attachments = draft.attachments;
    ws.pastes = draft.pastes;
    if switched_chat {
        // Another chat's draft: Cmd+Z must not bring back the text of the chat just left.
        ws.composer.update(cx, |composer, cx| {
            composer.set_value(draft.text, window, cx)
        });
    } else if edited {
        ws.write_composer(&draft.text, window, cx);
    }
}

pub fn render(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    // Subagents may go on after the chat's own turn is over; Stop ends them too.
    let working = ws
        .shown_chat()
        .is_some_and(|chat| chat.working || chat.agents.values().any(|agent| agent.working()));
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
    let hint = suggest::tab_hint(ws, cx);
    // The box itself marks a suggestion: its edge takes the accent while one is offered,
    // and flares once as Tab takes it in.
    let light = suggest::edge_light(ws, window, cx);
    let edge = match light {
        Some(strength) => accent.opacity(strength),
        None if writing => accent.opacity(0.5),
        None => cx.theme().border,
    };
    let band = panel::panel(ws, edge, cx);
    let tab = panel::tab(ws, edge, window, cx);
    // One card: what goes with the message on top, the text under it across the whole
    // width, and a row of controls at the bottom. The card grows upward, as the box sits
    // at the foot of the chat.
    let mut lift = shadows(Level::Raised, cx);
    // Under the band the box is the lower half of one shape: its top edge is not lit.
    if band.is_some() {
        lift.retain(|shadow| !shadow.inset);
    }
    // The box lights up a little while it is written in, more while it flares.
    if writing || light.is_some() {
        let glow = light.map_or(0.14, |strength| {
            0.14 + 0.16 * (strength - 0.5).max(0.) / 0.5
        });
        lift.push(BoxShadow {
            color: accent.opacity(glow),
            offset: point(px(0.), px(0.)),
            blur_radius: px(18.),
            spread_radius: px(0.),
            inset: false,
        });
    }
    let card = div()
        .flex()
        .flex_col()
        .rounded_2xl()
        .when(band.is_some(), |card| card.rounded_t_none())
        .elevation(Level::Raised, cx)
        .border_color(edge)
        .shadow(lift)
        .relative()
        .children(mentions.map(crate::ui::widgets::on_top))
        .children(chips)
        .child(
            div()
                .relative()
                .w_full()
                .px_2()
                .pt_1()
                // The suggestion's line stops short of the Tab hint at its end.
                .when(hint.is_some(), |row| row.pr(px(200.)))
                .child(Textarea::new(&ws.composer).appearance(false))
                .children(hint),
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
        );
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
                            // The panel, when open, stands on the card and takes its upper
                            // edge; the tab stands on whichever is on top.
                            // Its room is padding, so the pointer on the tab is still
                            // over the box and the tab stays while it is reached for.
                            div()
                                .id("composer-stack")
                                .relative()
                                .flex()
                                .flex_col()
                                .when(tab.is_some(), |stack| stack.pt(px(panel::TAB.1 - 1.)))
                                .on_hover(cx.listener(|ws, hovered: &bool, _, cx| {
                                    ws.dock.box_hovered = *hovered;
                                    ws.dock.tab_seen |= *hovered;
                                    cx.notify();
                                }))
                                .children(band)
                                .child(card)
                                .children(tab),
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
            // Shift+Tab over the empty box switches the mode, whatever is suggested.
            if keys.key == "tab"
                && keys.modifiers.shift
                && !keys.modifiers.control
                && ws.composer.read(cx).value().is_empty()
            {
                cx.stop_propagation();
                ws.next_mode(cx);
            } else if keys.key == "tab" && !keys.modifiers.shift && !keys.modifiers.control {
                cx.stop_propagation();
                // Over a mention or a command, Tab completes it, as in the TUI; over the
                // empty box it takes in the suggestion shown there.
                if ws.accept_suggestion(window, cx) {
                    // Taken in.
                } else if let Some(name) = ws.mention_matches(cx).first().cloned() {
                    ws.complete_mention(name, cx);
                } else if let Some((name, _)) = ws.command_matches(cx).first().copied() {
                    ws.complete_command(name, window, cx);
                } else {
                    ws.next_mode(cx);
                }
            }
        }))
}

impl Workspace {
    /// Puts `text` in the box as one edit Cmd+Z takes back, the caret after it;
    /// `set_value` would wipe the box's history. Leaves the box alone when it has the text.
    pub fn write_composer(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.composer.update(cx, |composer, cx| {
            if composer.value() == text {
                return;
            }
            let len = composer.value().len();
            composer.set_selected_range(0..len, cx);
            composer.replace(text.to_owned(), window, cx);
        });
    }

    pub fn edit_draft(
        &mut self,
        thread_id: Option<String>,
        change: Change,
        cx: &mut Context<Self>,
    ) {
        let generation = self.drafts.generation();
        self.edit_draft_at(thread_id, generation, change, cx);
    }

    pub fn edit_draft_at(
        &mut self,
        thread_id: Option<String>,
        generation: u64,
        change: Change,
        cx: &mut Context<Self>,
    ) {
        self.fill_composer.push(Edit {
            thread_id,
            generation,
            change,
        });
        cx.notify();
    }
}
