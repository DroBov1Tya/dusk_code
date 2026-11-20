//! Reply buttons: pills under the answer that send their label as the user's next message,
//! the same as typing it. Two things are drawn this way: a `Choices` block of the model and
//! a light `ask_user` question (`approval.rs`).
//!
//! A pill is outline, 28 px high and round all the way; the app's own buttons are 8 px
//! round, 32 px, and only they are ever filled or danger coloured. A pill is never either,
//! whatever its label says, so a model cannot dress one up as an approval.

use crate::ui::press::Press;
use std::rc::Rc;

use dusk_protocol::thread::Choices;
use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::transcript::{Item, Transcript},
    ui::Type,
    workspace::Workspace,
};

const HEIGHT: f32 = 28.;

/// What a click on the pill at an index does; the index after the last label is "Other...".
pub type Pick = Rc<dyn Fn(&mut Workspace, usize, &mut Window, &mut Context<Workspace>)>;

pub fn render(
    ws: &Workspace,
    shown: &str,
    at: usize,
    choices: &Choices,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let labels: Vec<String> = choices.labels().iter().map(ToString::to_string).collect();
    let live = live(ws, shown, at);
    let thread = shown.to_owned();
    let sent = labels.clone();
    let pick: Pick = Rc::new(move |ws, index, _, cx| {
        if let Some(label) = sent.get(index) {
            ws.reply(&thread, label.clone(), cx);
        }
    });
    // Up to the answer above it: the row's own gap is added to the one the list gives.
    pills(
        &format!("reply-{shown}-{at}"),
        &labels,
        live,
        false,
        pick,
        cx,
    )
    .mt(px(-8.))
    .into_any_element()
}

/// Whether the buttons of the block at item `at` of chat `shown` take a click: the turn has
/// ended, nothing but the end of the same answer follows, and no click is on its way. A
/// chat that is not in `chats`, such as a subagent's in its window, is read only.
pub fn live(ws: &Workspace, shown: &str, at: usize) -> bool {
    live_in(ws.chats.get(shown), &ws.replied, shown, at)
}

fn live_in(
    chat: Option<&Transcript>,
    replied: &Option<(String, usize)>,
    shown: &str,
    at: usize,
) -> bool {
    chat.is_some_and(|chat| {
        settled(chat, at) && *replied != Some((shown.to_owned(), chat.items.len()))
    })
}

/// The turn is over and only more of the answer comes after item `at`: a user's message,
/// a notice or another block means the moment passed.
pub fn settled(chat: &Transcript, at: usize) -> bool {
    !chat.working
        && chat.items.get(at + 1..).is_some_and(|rest| {
            rest.iter()
                .all(|item| matches!(item, Item::Assistant { .. }))
        })
}

/// The caption "Reply" and a pill for each label, then "Other..." when asked for. Disabled
/// pills are dimmed, take no pointer and are not in the Tab order.
pub fn pills(
    id: &str,
    labels: &[String],
    live: bool,
    other: bool,
    pick: Pick,
    cx: &mut Context<Workspace>,
) -> Div {
    let theme = cx.theme();
    let (muted, ring, raised) = (theme.muted_foreground, theme.ring, theme.secondary);
    let (pressed, rule) = (theme.primary.opacity(0.12), theme.foreground.opacity(0.14));
    // The palette's secondary text, which the theme keeps under `info`.
    let text = theme.info;
    let focus = vec![BoxShadow {
        color: ring,
        offset: point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(1.),
        inset: false,
    }];
    let last = labels.len() + usize::from(other) - 1;
    let entries = labels
        .iter()
        .map(String::as_str)
        .chain(other.then_some("Other..."));
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap_2()
        .child(div().meta().text_color(muted).child("Reply"))
        .children(entries.enumerate().map(|(index, label)| {
            let pill = div()
                .id(SharedString::from(format!("{id}-{index}")))
                .h(px(HEIGHT))
                .px_3()
                .flex()
                .items_center()
                // A button's own corner, as the box's controls have: a full round read as a
                // tag, not as something to press.
                .rounded_lg()
                .border_1()
                .border_color(rule)
                .small()
                .font_weight(FontWeight::MEDIUM)
                .text_color(text)
                .whitespace_nowrap()
                .child(label.to_owned());
            if !live {
                return pill.opacity(0.45);
            }
            let pick = pick.clone();
            let focus = focus.clone();
            pill.cursor_pointer()
                .tab_stop(true)
                .tab_index(0)
                .hover(move |pill| pill.bg(raised))
                .press(pressed, cx)
                .focus_visible(move |pill| pill.border_color(ring).shadow(focus))
                // Left and Right move along the row and stop at its ends.
                .on_key_down(cx.listener(move |_, event: &KeyDownEvent, window, cx| {
                    match event.keystroke.key.as_str() {
                        "left" if index > 0 => window.focus_prev(cx),
                        "right" if index < last => window.focus_next(cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                }))
                .on_click(cx.listener(move |ws, _, window, cx| pick(ws, index, window, cx)))
        }))
}

impl Workspace {
    /// Sends `text` to the chat as the user's next message, once for the state of the chat
    /// the buttons were drawn in: the buttons stay disabled until the message comes back.
    fn reply(&mut self, thread_id: &str, text: String, cx: &mut Context<Self>) {
        let Some(items) = self.chats.get(thread_id).map(|chat| chat.items.len()) else {
            return;
        };
        if self.replied == Some((thread_id.to_owned(), items)) {
            return;
        }
        self.replied = Some((thread_id.to_owned(), items));
        let thread_id = thread_id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.threads().send(&thread_id, text, false).await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.replied = None;
                    ws.fail(error, cx);
                }
            },
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::{
        model::ModelEvent,
        thread::{Block, ThreadEvent},
    };

    use super::{live_in, settled};
    use crate::pages::chat::transcript::{Item, Transcript};

    fn choices() -> Item {
        let block: Block =
            serde_json::from_value(serde_json::json!({ "choices": { "labels": ["Yes", "No"] } }))
                .unwrap();
        Item::Block(block)
    }

    fn chat(working: bool, items: Vec<Item>) -> Transcript {
        let mut chat = Transcript::default();
        chat.working = working;
        chat.items = items;
        chat
    }

    fn user() -> Item {
        Item::User {
            text: "ok".into(),
            sequence: Some(1),
            answered: Vec::new(),
        }
    }

    #[test]
    fn buttons_are_live_only_when_last_and_the_turn_ended() {
        assert!(settled(&chat(false, vec![choices()]), 0));
        // The turn still runs.
        assert!(!settled(&chat(true, vec![choices()]), 0));
        // Any later message, a notice or a block means the moment passed.
        assert!(!settled(&chat(false, vec![choices(), user()]), 0));
        assert!(!settled(
            &chat(false, vec![choices(), Item::Notice("n".into())]),
            0
        ));
        assert!(!settled(&chat(false, vec![choices(), choices()]), 0));
        // The end of the same answer, written after the call, does not.
        let closing = Item::Assistant {
            text: "Shall I?".into(),
            reasoning: String::new(),
        };
        assert!(settled(&chat(false, vec![choices(), closing]), 0));
        assert!(!settled(&chat(false, vec![]), 0));
    }

    #[test]
    fn one_press_disables_them_until_the_message_comes_back() {
        let mut thread = chat(false, vec![choices()]);
        let none = None;
        assert!(live_in(Some(&thread), &none, "t", 0));
        let pressed = Some(("t".to_owned(), 1));
        assert!(!live_in(Some(&thread), &pressed, "t", 0));
        thread.apply(ThreadEvent::Model(ModelEvent::Text("later".into())));
        assert!(live_in(Some(&thread), &pressed, "t", 0));
    }

    #[test]
    fn buttons_of_a_chat_the_window_does_not_hold_are_disabled() {
        // A subagent's thread is drawn from its parent's chat and is not in `chats`.
        assert!(!live_in(None, &None, "agent", 0));
    }
}
