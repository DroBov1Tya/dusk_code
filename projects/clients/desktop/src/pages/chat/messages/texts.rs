//! The markdown of the shown chat's messages and answers, each parsed once and kept while
//! the chat is shown. A text view made from its words keeps its parse only while it is
//! drawn frame after frame, and one past 4 KiB parses again off the main thread, so a row
//! scrolled back into sight laid out empty for a frame: the list took that for the row's
//! height and the next scroll landed a long way from where the reader was. Kept here, a
//! row comes back at its real height, and when a parse lands its row is measured again.

use std::{cell::RefCell, collections::HashMap};

use gpui_kit::{
    component::text::{TextView, TextViewState},
    *,
};

use super::Row;
use crate::{pages::chat::links, workspace::Workspace};

/// Kept behind a cell so rows drawn from a shared `Workspace`, such as an agent's report
/// inside an unfolded activity, keep their texts too.
#[derive(Default)]
pub struct Texts {
    kept: RefCell<HashMap<SharedString, Kept>>,
}

struct Kept {
    state: Entity<TextViewState>,
    source: Source,
    /// Measures the row again when the state changes, as when a long parse lands.
    _changed: Subscription,
}

impl Texts {
    /// A message's words, under `key`, for item `item` of the chat.
    pub fn message(
        &self,
        key: SharedString,
        item: usize,
        text: &str,
        cx: &mut Context<Workspace>,
    ) -> TextView {
        links::kept(&self.state(key, item, text, false, cx), false)
    }

    /// An answer's words, with its labels, as `message`.
    pub fn answer(
        &self,
        key: SharedString,
        item: usize,
        text: &str,
        cx: &mut Context<Workspace>,
    ) -> TextView {
        links::kept(&self.state(key, item, text, true, cx), true)
    }

    fn state(
        &self,
        key: SharedString,
        item: usize,
        text: &str,
        marked: bool,
        cx: &mut Context<Workspace>,
    ) -> Entity<TextViewState> {
        let known = self.kept.borrow_mut().get_mut(&key).map(|kept| {
            let changed = kept.source.take(text, marked);
            (kept.state.clone(), changed)
        });
        if let Some((state, changed)) = known {
            if changed {
                // Grown words are parsed as an append.
                let source = links::linkify(text, marked);
                state.update(cx, |state, cx| state.set_text(&source, cx));
            }
            return state;
        }
        let source = links::linkify(text, marked);
        let state = cx.new(|cx| TextViewState::markdown(&source, cx));
        let changed = cx.observe(&state, move |ws, _, cx| ws.text_changed(item, cx));
        self.kept.borrow_mut().insert(
            key,
            Kept {
                state: state.clone(),
                source: Source {
                    raw: text.to_owned(),
                    marked,
                },
                _changed: changed,
            },
        );
        state
    }
}

/// The words a state was last given, before links were made from them. On most frames
/// they come again unchanged, and then nothing is made or compared but these.
struct Source {
    raw: String,
    marked: bool,
}

impl Source {
    /// Keeps `text` and says whether it differs from what was kept, so the links must be
    /// made again.
    fn take(&mut self, text: &str, marked: bool) -> bool {
        if self.marked == marked && self.raw == text {
            return false;
        }
        self.raw.clear();
        self.raw.push_str(text);
        self.marked = marked;
        true
    }
}

impl Workspace {
    /// Measures again the row that shows item `item`: its own, or the activity it is in.
    fn text_changed(&mut self, item: usize, cx: &mut Context<Self>) {
        let shows = |row: &Row| match *row {
            Row::Item(at) => at == item,
            Row::Activity { start, end, .. } => (start..end).contains(&item),
            Row::Working => false,
        };
        if let Some(row) = self.rows.iter().position(shows) {
            self.list.remeasure_items(row..row + 1);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Source;

    #[test]
    fn links_are_made_again_only_when_the_words_change() {
        let mut source = Source {
            raw: "Read `src/main.rs`".to_owned(),
            marked: false,
        };
        assert!(!source.take("Read `src/main.rs`", false));
        // Streamed words grow, and an answer reads its labels.
        assert!(source.take("Read `src/main.rs` now", false));
        assert!(!source.take("Read `src/main.rs` now", false));
        assert!(source.take("Read `src/main.rs` now", true));
        assert_eq!(source.raw, "Read `src/main.rs` now");
    }
}
