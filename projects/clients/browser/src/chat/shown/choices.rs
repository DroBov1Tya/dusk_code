//! Reply buttons: a few short labels under an answer, each one a click that sends the
//! label as the user's next message. They come from a `Choices` block or from a light
//! `ask_user` question and look alike. A pill is never filled and never danger coloured
//! (app buttons are), and it approves nothing: the click only says words the user sees
//! as their own bubble.

use crate::{
    chat::data::{ChatData, Item},
    state::{Chat, state},
};
use dusk_protocol::thread::Choices;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

/// A block of reply buttons under the answer.
#[component]
pub fn ChoicesView(
    choices: Choices,
    id: u64,
    chat: Chat,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    let app = state();
    let labels: Vec<String> = choices.labels().iter().map(ToString::to_string).collect();
    // Between the click and the core taking the message into the chat, nothing says it
    // was sent, so a second click would send it twice.
    let clicked = RwSignal::new(false);
    Effect::new(move |_| {
        if chat.with(|c| !c.pending.is_empty()) {
            clicked.set(false);
        }
    });
    let live = Signal::derive(move || !read_only && !clicked.get() && chat.with(|c| live(c, id)));
    let pick = Callback::new({
        let labels = labels.clone();
        move |at: usize| {
            let Some(label) = labels.get(at).cloned() else {
                return;
            };
            clicked.set(true);
            app.send(thread.clone(), label, false, Vec::new());
        }
    });
    view! { <ReplyRow labels live pick /> }
}

/// Whether the reply buttons of the item `id` can be pressed: only while that item is
/// the last of the chat and nothing is running or on its way to the core. After any later
/// message they stay in sight, disabled, as a record of what was offered.
pub fn live(chat: &ChatData, id: u64) -> bool {
    !chat.working
        && chat.pending.is_empty()
        && chat
            .items
            .len()
            .checked_sub(1)
            .is_some_and(|last| chat.id(last) == id && matches!(chat.items[last], Item::Block(_)))
}

/// The labels a question is drawn with as reply buttons: none when it is not light, and
/// then it stays the card.
pub fn question_labels(question: &dusk_protocol::tools::Question) -> Option<Vec<String>> {
    question
        .light()
        .then(|| question.options.iter().map(|o| plain(o)).collect())
}

/// A label for display: the model's text without control and text-direction characters,
/// which could make a button read as something else. Labels from blocks are already so.
fn plain(text: &str) -> String {
    text.trim()
        .chars()
        .filter(|c| {
            !c.is_control()
                && !matches!(c, '\u{2028}' | '\u{2029}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .collect()
}

/// The row: "Reply" and a pill for each label, then "Other..." when `other` is given.
#[component]
pub fn ReplyRow(
    labels: Vec<String>,
    #[prop(into)] live: Signal<bool>,
    /// Called with the index of the pressed label.
    pick: Callback<usize>,
    #[prop(optional)] other: Option<Callback<()>>,
) -> impl IntoView {
    let move_focus = move |ev: leptos::ev::KeyboardEvent| {
        let forward = match ev.key().as_str() {
            "ArrowRight" => true,
            "ArrowLeft" => false,
            _ => return,
        };
        let Some(button) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        else {
            return;
        };
        let mut next = step(&button, forward);
        while let Some(element) = next {
            if element.tag_name() == "BUTTON" && !element.has_attribute("disabled") {
                if let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() {
                    ev.prevent_default();
                    let _ = element.focus();
                }
                return;
            }
            next = step(&element, forward);
        }
    };
    view! {
        <div class="reply" role="group" aria-label="Reply" on:keydown=move_focus>
            <span class="meta muted">"Reply"</span>
            {labels.into_iter().enumerate().map(|(at, label)| {
                let described = format!("Reply: {label}");
                view! {
                <button
                    class="pill"
                    disabled=move || !live.get()
                    aria-label=described
                    on:click=move |_| pick.run(at)
                >{label}</button>
            }}).collect_view()}
            {other.map(|other| view! {
                <button class="pill" disabled=move || !live.get() on:click=move |_| other.run(())>
                    "Other\u{2026}"
                </button>
            })}
        </div>
    }
}

fn step(element: &web_sys::Element, forward: bool) -> Option<web_sys::Element> {
    if forward {
        element.next_element_sibling()
    } else {
        element.previous_element_sibling()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_protocol::{
        thread::{Block, Line},
        tools::Question,
    };

    fn choices() -> Block {
        Block::Choices(
            Choices::new(vec![Line::new("Yes").unwrap(), Line::new("No").unwrap()]).unwrap(),
        )
    }

    fn question(options: &[&str], multiple: bool) -> Question {
        Question {
            question: "Which?".into(),
            options: options.iter().map(|o| (*o).to_owned()).collect(),
            multiple,
        }
    }

    fn chat_with_block() -> ChatData {
        let mut chat = ChatData::default();
        chat.push(Item::Block(choices()), 0);
        chat
    }

    #[test]
    fn a_light_question_becomes_buttons_and_another_stays_a_card() {
        assert_eq!(
            question_labels(&question(&["Yes", "No"], false)),
            Some(vec!["Yes".to_owned(), "No".to_owned()])
        );
        assert_eq!(question_labels(&question(&["Yes", "No"], true)), None);
        assert_eq!(question_labels(&question(&["Yes"], false)), None);
        let long = "x".repeat(25);
        assert_eq!(question_labels(&question(&[&long, "No"], false)), None);
    }

    #[test]
    fn hostile_option_text_stays_text_without_control_or_direction_characters() {
        let labels = question_labels(&question(&["<b>Yes</b>\u{202e}", "No\n"], false)).unwrap();
        assert_eq!(labels, ["<b>Yes</b>", "No"]);
    }

    #[test]
    fn buttons_are_live_only_as_the_last_item_of_a_quiet_chat() {
        let mut chat = chat_with_block();
        assert!(live(&chat, 1));
        assert!(!live(&chat, 2));
        chat.working = true;
        assert!(!live(&chat, 1));
        chat.working = false;
        chat.sent("hello".into());
        assert!(!live(&chat, 1));
        chat.pending.clear();
        chat.push(
            Item::User {
                text: "later".into(),
                sequence: None,
                answered: Vec::new(),
            },
            1,
        );
        assert!(!live(&chat, 1));
    }
}
