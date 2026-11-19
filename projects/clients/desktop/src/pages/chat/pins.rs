//! The pins of the chat: a thin handle on the transcript's upper edge says how many there
//! are, and a click pulls a drawer of them down over the chat; a row goes to its message.
//! Excerpts are the core's plain text.

use crate::ui::elevation::{Level, shadows};
use crate::ui::press::Press;
use std::time::{Duration, Instant};

use dusk_protocol::thread::{MessageRole, Pin};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::messages::Row;
use crate::{
    ui::{Type, icons, widgets, widgets::Focusable},
    workspace::Workspace,
};

/// The handle's height and a row's in the drawer.
const HANDLE: f32 = 22.;
const ROW: f32 = 32.;
/// The drawer scrolls past this many rows.
const SHOWN_ROWS: f32 = 8.;
/// How long the message gone to stays tinted.
const FLASH: Duration = Duration::from_millis(1200);

#[derive(Default)]
pub struct PinsView {
    /// The pin the key goes to next, by its place among the chat's pins.
    pub at: usize,
    /// The drawer is pulled down.
    pub open: bool,
    /// A pin or an unpin waits for the core.
    pub busy: bool,
    /// The chat and item gone to last, and when, for its tint.
    pub flash: Option<(String, usize, Instant)>,
}

fn shown_pins(ws: &Workspace) -> Option<(&str, &[Pin])> {
    let shown = ws.shown.as_deref()?;
    let pins = &ws.chats.get(shown)?.pins;
    (!pins.is_empty()).then_some((shown, pins.as_slice()))
}

/// The handle over the transcript, none while the chat has no pins.
pub fn handle(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let (_, pins) = shown_pins(ws)?;
    let theme = cx.theme();
    let (surface, muted, accent, border, raised) = (
        theme.muted,
        theme.muted_foreground,
        theme.primary,
        theme.border,
        theme.list_hover,
    );
    let open = ws.pins.open;
    Some(
        div()
            .id("pins-handle")
            .focusable_row(cx)
            .role(Role::Button)
            .aria_label(if open {
                "Fold the pinned messages"
            } else {
                "Show the pinned messages"
            })
            .flex_none()
            .h(px(HANDLE))
            .flex()
            .items_center()
            .justify_center()
            .gap_1()
            .bg(surface)
            .border_b_1()
            .border_color(border)
            .cursor_pointer()
            .hover(move |bar| bar.bg(raised))
            .press(raised, cx)
            .child(Icon::default().path(icons::PIN).xsmall().text_color(accent))
            .child(
                div()
                    .meta()
                    .text_color(muted)
                    .child(format!("{} pinned", pins.len())),
            )
            .child(
                Icon::new(if open {
                    IconName::ChevronUp
                } else {
                    IconName::ChevronDown
                })
                .xsmall()
                .text_color(muted),
            )
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.pins.open = !ws.pins.open;
                cx.notify();
            })),
    )
}

/// The drawer of the pins over the top of the transcript, while it is pulled down.
pub fn drawer(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    if !ws.pins.open {
        return None;
    }
    let (shown, pins) = shown_pins(ws)?;
    let theme = cx.theme();
    let (surface, border) = (theme.muted, theme.border);
    let list = div()
        .id("pins-drawer")
        .max_h(px(ROW * SHOWN_ROWS + 8.))
        .overflow_y_scroll()
        .flex()
        .flex_col()
        .px_2()
        .py_1()
        .bg(surface)
        .border_b_1()
        .border_color(border)
        .shadow(shadows(Level::Floating, cx))
        .children(
            pins.iter()
                .enumerate()
                .map(|(n, pin)| list_row(ws, shown, n, pin, cx)),
        );
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .occlude()
            .child(widgets::unfold("pins-drawer-open", list)),
    )
}

fn list_row(
    ws: &Workspace,
    shown: &str,
    n: usize,
    pin: &Pin,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, raised) = (theme.muted_foreground, theme.list_hover);
    let who = match pin.role {
        MessageRole::User => "You",
        _ => "Agent",
    };
    let group = SharedString::from(format!("pin-row-{n}"));
    let (thread_id, message_id) = (shown.to_owned(), pin.message_id.clone());
    div()
        .id(("pin", n))
        .group(group.clone())
        .focusable_row(cx)
        .role(Role::Button)
        .aria_label(format!("{who}: {}", pin.excerpt))
        .h(px(ROW))
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .hover(move |row| row.bg(raised))
        .press(raised, cx)
        .child(
            div()
                .flex_none()
                .min_w(px(48.))
                .meta()
                .text_color(muted)
                .child(who),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .small()
                .truncate()
                .child(pin.excerpt.to_string()),
        )
        .child(
            Button::new(SharedString::from(format!("unpin-{n}")))
                .ghost()
                .xsmall()
                .icon(IconName::Close)
                .tooltip("Unpin")
                .disabled(ws.pins.busy)
                .invisible()
                .group_hover(group, |button| button.visible())
                .on_click(cx.listener(move |ws, _, _, cx| {
                    cx.stop_propagation();
                    ws.set_pinned(&thread_id, &message_id, false, cx);
                })),
        )
        .on_click(cx.listener(move |ws, _, _, cx| {
            ws.go_to_pin(n, cx);
            ws.pins.at = n;
            ws.pins.open = false;
            cx.notify();
        }))
}

impl Workspace {
    /// Goes to the shown chat's pin `n`: the list scrolls to its message, tinted for a
    /// moment. A message a compaction folded away is said to be so instead.
    pub fn go_to_pin(&mut self, n: usize, cx: &mut Context<Self>) {
        let Some(shown) = self.shown.clone() else {
            return;
        };
        let Some(chat) = self.chats.get(&shown) else {
            return;
        };
        let Some(pin) = chat.pins.get(n) else {
            return;
        };
        if pin.compacted {
            self.toast(
                "That message was folded away by a compaction".to_owned(),
                cx,
            );
            return;
        }
        let item = chat.item_of(&pin.message_id);
        let row = item.and_then(|item| {
            self.rows.iter().position(|row| match *row {
                Row::Item(at) => at == item,
                Row::Activity { start, end, .. } => (start..end).contains(&item),
                Row::Working => false,
            })
        });
        let (Some(item), Some(row)) = (item, row) else {
            self.toast("That message is not shown here".to_owned(), cx);
            return;
        };
        self.list.scroll_to(ListOffset {
            item_ix: row,
            offset_in_item: px(0.),
        });
        let since = Instant::now();
        self.pins.flash = Some((shown, item, since));
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FLASH).await;
            let _ = this.update(cx, |ws, cx| {
                if ws
                    .pins
                    .flash
                    .as_ref()
                    .is_some_and(|(_, _, at)| *at == since)
                {
                    ws.pins.flash = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// The next pin of the shown chat, from the key.
    pub fn next_pin(&mut self, cx: &mut Context<Self>) {
        let count = self
            .shown
            .as_ref()
            .and_then(|shown| self.chats.get(shown))
            .map_or(0, |chat| chat.pins.len());
        if count == 0 {
            return;
        }
        let at = self.pins.at % count;
        self.go_to_pin(at, cx);
        self.pins.at = at + 1;
        cx.notify();
    }

    /// Pins or unpins a message; the button waits until the core answers.
    pub fn set_pinned(
        &mut self,
        thread_id: &str,
        message_id: &str,
        pinned: bool,
        cx: &mut Context<Self>,
    ) {
        if self.pins.busy {
            return;
        }
        self.pins.busy = true;
        let (thread_id, message_id) = (thread_id.to_owned(), message_id.to_owned());
        let id = thread_id.clone();
        self.core.call(
            cx,
            move |client| async move { client.threads().pin(&thread_id, &message_id, pinned).await },
            move |ws, result, cx| {
                ws.pins.busy = false;
                match result {
                    Ok(pins) => {
                        if let Some(chat) = ws.chats.get_mut(&id) {
                            chat.pins = pins;
                        }
                        cx.notify();
                    }
                    // Such as "Unpin one first": a note, not an error to dismiss.
                    Err(error) => ws.toast(format!("{error:#}"), cx),
                }
            },
        );
        cx.notify();
    }

    /// Whether item `at` of `chat` was just gone to, for its tint.
    pub fn flashed(&self, chat: &str, at: usize) -> bool {
        self.pins
            .flash
            .as_ref()
            .is_some_and(|(shown, item, _)| shown == chat && *item == at)
    }
}

/// The pin or unpin button of item `at`, among the tools of its row; none for an item
/// whose stored message is not known yet, as an answer still being written.
pub fn button(
    ws: &Workspace,
    shown: &str,
    at: usize,
    cx: &mut Context<Workspace>,
) -> Option<Button> {
    let chat = ws.chats.get(shown)?;
    let message_id = chat.message_ids.get(&at)?.clone();
    let pinned = chat.pinned(at);
    let thread_id = shown.to_owned();
    Some(
        Button::new(SharedString::from(format!("pin-{shown}-{at}")))
            .ghost()
            .small()
            .icon(Icon::default().path(icons::PIN))
            .when(pinned, |button| {
                use gpui_kit::component::Selectable;
                button.selected(true)
            })
            .tooltip(if pinned { "Unpin" } else { "Pin" })
            .disabled(ws.pins.busy)
            .on_click(
                cx.listener(move |ws, _, _, cx| {
                    ws.set_pinned(&thread_id, &message_id, !pinned, cx)
                }),
            ),
    )
}

/// The fork button of item `at`: a new chat from the history up to it. None while its
/// stored message is not known yet.
pub fn fork_button(
    ws: &Workspace,
    shown: &str,
    at: usize,
    cx: &mut Context<Workspace>,
) -> Option<Button> {
    let message_id = ws.chats.get(shown)?.message_ids.get(&at)?.clone();
    let thread_id = shown.to_owned();
    Some(
        Button::new(SharedString::from(format!("fork-{shown}-{at}")))
            .ghost()
            .small()
            .icon(Icon::default().path(icons::GIT_FORK))
            .tooltip("Fork from here")
            .on_click(cx.listener(move |ws, _, _, cx| ws.fork_at(&thread_id, &message_id, cx))),
    )
}
