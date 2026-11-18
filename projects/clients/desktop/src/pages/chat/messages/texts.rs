//! The markdown of the shown chat's messages and answers, each parsed once and kept while
//! the chat is shown. A text view made from its words keeps its parse only while it is
//! drawn frame after frame, and one past 4 KiB parses again off the main thread, so a row
//! scrolled back into sight laid out empty for a frame: the list took that for the row's
//! height and the next scroll landed a long way from where the reader was. Kept here, a
//! row comes back at its real height, and when a parse lands its row is measured again.

use std::collections::HashMap;

use gpui_kit::{
    component::text::{TextView, TextViewState},
    *,
};

use super::Row;
use crate::{pages::chat::links, workspace::Workspace};

#[derive(Default)]
pub struct Texts {
    kept: HashMap<SharedString, Kept>,
}

struct Kept {
    state: Entity<TextViewState>,
    /// Measures the row again when the state changes, as when a long parse lands.
    _changed: Subscription,
}

impl Texts {
    /// A message's words, under `key`, for item `item` of the chat.
    pub fn message(
        &mut self,
        key: SharedString,
        item: usize,
        text: &str,
        cx: &mut Context<Workspace>,
    ) -> TextView {
        links::kept(
            &self.state(key, item, links::linkify(text, false), cx),
            false,
        )
    }

    /// An answer's words, with its labels, as `message`.
    pub fn answer(
        &mut self,
        key: SharedString,
        item: usize,
        text: &str,
        cx: &mut Context<Workspace>,
    ) -> TextView {
        links::kept(&self.state(key, item, links::linkify(text, true), cx), true)
    }

    fn state(
        &mut self,
        key: SharedString,
        item: usize,
        source: String,
        cx: &mut Context<Workspace>,
    ) -> Entity<TextViewState> {
        if let Some(kept) = self.kept.get(&key) {
            // Unchanged words cost a comparison; grown ones are parsed as an append.
            kept.state
                .update(cx, |state, cx| state.set_text(&source, cx));
            return kept.state.clone();
        }
        let state = cx.new(|cx| TextViewState::markdown(&source, cx));
        let changed = cx.observe(&state, move |ws, _, cx| ws.text_changed(item, cx));
        self.kept.insert(
            key,
            Kept {
                state: state.clone(),
                _changed: changed,
            },
        );
        state
    }
}

impl Workspace {
    fn text_changed(&mut self, item: usize, cx: &mut Context<Self>) {
        if let Some(row) = self.rows.iter().position(|row| *row == Row::Item(item)) {
            self.list.remeasure_items(row..row + 1);
            cx.notify();
        }
    }
}
