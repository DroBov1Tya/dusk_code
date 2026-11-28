//! What is typed and attached but not sent, kept for each chat while another one is shown.
//! Only in memory: a draft is gone with the app.

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// The unsent message of one chat.
#[derive(Default, Debug, PartialEq)]
pub struct Draft {
    pub text: String,
    pub attachments: Vec<PathBuf>,
    pub pastes: Vec<String>,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.attachments.is_empty() && self.pastes.is_empty()
    }

    /// The message as it goes to the core: the text, each long paste wrapped, then the
    /// attached files as their paths, which the core keeps among the chat's sources.
    pub fn flat(&self) -> String {
        let mut text = self.text.trim().to_owned();
        for pasted in &self.pastes {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&dusk_protocol::thread::wrap(pasted));
        }
        for path in &self.attachments {
            if !text.is_empty() {
                text.push('\n');
            }
            // A path with spaces goes quoted, so the core reads it as one.
            let path = path.display().to_string();
            match path.contains(char::is_whitespace) && !path.contains('"') {
                true => text.push_str(&format!("\"{path}\"")),
                false => text.push_str(&path),
            }
        }
        text
    }
}

pub enum Change {
    #[cfg(feature = "screenshot")]
    Replace(String),
    Append(String),
    Attach(Vec<PathBuf>),
    Restore(Draft),
}

pub struct Edit {
    pub thread_id: Option<String>,
    pub generation: u64,
    pub change: Change,
}

impl Draft {
    fn apply(&mut self, change: Change) {
        match change {
            #[cfg(feature = "screenshot")]
            Change::Replace(text) => self.text = text,
            Change::Append(text) => {
                self.text = match self.text.trim_end() {
                    "" => text,
                    before => format!("{before} {text}"),
                };
            }
            Change::Attach(paths) => self.attachments.extend(paths),
            Change::Restore(mut draft) => {
                if !self.text.is_empty() {
                    if !draft.text.is_empty() {
                        draft.text.push_str("\n\n");
                    }
                    draft.text.push_str(&self.text);
                }
                draft.attachments.append(&mut self.attachments);
                draft.pastes.append(&mut self.pastes);
                *self = draft;
            }
        }
    }
}

/// The drafts of the chats that are not shown. A chat is its thread ID; `None` is the chat
/// about to start, which has no thread yet.
#[derive(Default)]
pub struct Drafts {
    /// The chat whose draft is in the composer.
    held: Option<String>,
    /// The held chat was deleted: what the composer has is not kept.
    dropped: bool,
    kept: HashMap<Option<String>, Draft>,
    forgotten: HashSet<String>,
    generation: u64,
    starting: Option<u64>,
    started: HashMap<u64, String>,
}

impl Drafts {
    /// The identity of the unsent chat currently being composed.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// A new unsent chat invalidates delayed work from the previous one.
    pub fn new_chat(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn starting(&self) -> bool {
        self.starting == Some(self.generation)
    }

    pub fn begin(&mut self) {
        self.starting = Some(self.generation);
    }

    pub fn finish(&mut self, generation: u64) {
        if self.starting == Some(generation) {
            self.starting = None;
        }
    }

    /// An asynchronous result belongs to the chat that started it, even between frames.
    pub fn edit(&mut self, current: &mut Draft, mut edit: Edit) {
        if edit.thread_id.is_none() {
            edit.thread_id = self.started.get(&edit.generation).cloned();
        }
        if edit.thread_id.is_none() && edit.generation != self.generation {
            return;
        }
        if edit
            .thread_id
            .as_ref()
            .is_some_and(|id| self.forgotten.contains(id))
        {
            return;
        }
        if !self.dropped && self.held == edit.thread_id {
            current.apply(edit.change);
        } else {
            self.kept
                .entry(edit.thread_id)
                .or_default()
                .apply(edit.change);
        }
    }

    /// Moves the composer to another chat: keeps `current` for the chat it was written in
    /// and gives the draft of `shown`. `None` when `shown` already has the composer.
    pub fn switch(
        &mut self,
        shown: Option<&str>,
        current: impl FnOnce() -> Draft,
    ) -> Option<Draft> {
        if !self.dropped && self.held.as_deref() == shown {
            return None;
        }
        if !std::mem::take(&mut self.dropped) {
            let current = current();
            if !current.is_empty() {
                self.kept.insert(self.held.take(), current);
            }
        }
        self.held = shown.map(str::to_owned);
        Some(self.kept.remove(&self.held).unwrap_or_default())
    }

    /// The chat about to start got its thread: what is typed since belongs to that thread.
    pub fn started(&mut self, thread_id: &str, generation: u64) -> bool {
        if generation != self.generation {
            return false;
        }
        self.started.insert(generation, thread_id.to_owned());
        if self.held.is_none() && !self.dropped {
            self.held = Some(thread_id.to_owned());
        } else if let Some(draft) = self.kept.remove(&None) {
            self.kept.insert(Some(thread_id.to_owned()), draft);
        }
        true
    }

    /// A deleted chat takes its draft with it.
    pub fn forget(&mut self, thread_id: &str) {
        self.forgotten.insert(thread_id.to_owned());
        self.kept.remove(&Some(thread_id.to_owned()));
        if self.held.as_deref() == Some(thread_id) {
            self.dropped = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(text: &str) -> Draft {
        Draft {
            text: text.into(),
            ..Draft::default()
        }
    }

    #[test]
    fn each_chat_keeps_its_own_draft() {
        let mut drafts = Drafts::default();
        // The same chat on another frame: the composer is left alone.
        assert_eq!(drafts.switch(None, || text("new")), None);
        assert_eq!(drafts.switch(Some("a"), || text("new")), Some(text("")));
        assert_eq!(drafts.switch(Some("b"), || text("for a")), Some(text("")));
        assert_eq!(
            drafts.switch(Some("a"), || text("for b")),
            Some(text("for a"))
        );
        // Sent: the composer is empty when the chat is left, so nothing comes back.
        assert_eq!(drafts.switch(Some("b"), || text("")), Some(text("for b")));
        assert_eq!(drafts.switch(Some("a"), || text("for b")), Some(text("")));
        assert_eq!(drafts.switch(None, || text("")), Some(text("new")));
    }

    #[test]
    fn a_started_chat_takes_the_composer_and_a_deleted_one_loses_its_draft() {
        let mut drafts = Drafts::default();
        drafts.started("a", 0);
        // The first message made thread `a`: the text typed after it is not a new chat's.
        assert_eq!(drafts.switch(Some("a"), || text("more")), None);
        assert_eq!(drafts.switch(Some("b"), || text("more")), Some(text("")));
        drafts.forget("a");
        drafts.forget("b");
        assert_eq!(drafts.switch(None, || text("for b")), Some(text("")));
        assert_eq!(drafts.switch(Some("a"), || text("")), Some(text("")));
        assert_eq!(drafts.switch(Some("b"), || text("")), Some(text("")));
    }
    #[test]
    fn delayed_edits_follow_their_chat_and_preserve_attachments() {
        let mut drafts = Drafts::default();
        drafts.switch(Some("a"), Draft::default);
        let mut current = text("for a");
        drafts.edit(
            &mut current,
            Edit {
                thread_id: Some("b".into()),
                generation: 0,
                change: Change::Restore(text("retry b")),
            },
        );
        drafts.edit(
            &mut current,
            Edit {
                thread_id: Some("a".into()),
                generation: 0,
                change: Change::Append("dictated".into()),
            },
        );
        drafts.edit(
            &mut current,
            Edit {
                thread_id: Some("a".into()),
                generation: 0,
                change: Change::Attach(vec!["/tmp/a.png".into()]),
            },
        );
        let b = drafts.switch(Some("b"), || current).unwrap();
        assert_eq!(b.text, "retry b");
        let a = drafts.switch(Some("a"), || b).unwrap();
        assert_eq!(a.text, "for a dictated");
        assert_eq!(a.attachments, [PathBuf::from("/tmp/a.png")]);
        drafts.forget("b");
        let mut a = a;
        drafts.edit(
            &mut a,
            Edit {
                thread_id: Some("b".into()),
                generation: 0,
                change: Change::Append("late".into()),
            },
        );
        assert_eq!(drafts.switch(Some("b"), || a), Some(text("")));
    }

    #[test]
    fn old_unsent_actions_do_not_change_a_replacement_draft() {
        let mut drafts = Drafts::default();
        let old = drafts.generation();
        drafts.new_chat();
        let mut current = text("replacement");
        drafts.edit(
            &mut current,
            Edit {
                thread_id: None,
                generation: old,
                change: Change::Append("late".into()),
            },
        );
        assert_eq!(current, text("replacement"));
        assert!(!drafts.started("old-thread", old));
        assert_eq!(drafts.switch(None, || text("replacement")), None);
        assert!(drafts.started("new-thread", drafts.generation()));
        assert_eq!(
            drafts.switch(Some("new-thread"), || text("replacement")),
            None
        );
    }

    #[test]
    fn late_unsent_edit_follows_the_created_thread() {
        let mut drafts = Drafts::default();
        let generation = drafts.generation();
        assert!(drafts.started("created", generation));
        drafts.new_chat();
        let mut current = text("replacement");
        assert_eq!(
            drafts.switch(None, || text("created draft")),
            Some(Draft::default())
        );
        drafts.edit(
            &mut current,
            Edit {
                thread_id: None,
                generation,
                change: Change::Append("dictated".into()),
            },
        );
        assert_eq!(current.text, "replacement");
        assert_eq!(
            drafts.switch(Some("created"), || current).unwrap().text,
            "created draft dictated"
        );
        drafts.forget("created");
        let mut current = Draft::default();
        drafts.edit(
            &mut current,
            Edit {
                thread_id: None,
                generation,
                change: Change::Append("after deletion".into()),
            },
        );
        assert_eq!(current, Draft::default());
    }

    #[test]
    fn retry_restores_the_sent_draft_without_erasing_new_typing() {
        let mut draft = text("new words");
        draft.apply(Change::Restore(Draft {
            text: "failed message".into(),
            attachments: vec!["/tmp/file".into()],
            pastes: vec!["large paste".into()],
        }));
        assert_eq!(draft.text, "failed message\n\nnew words");
        assert_eq!(draft.attachments.len(), 1);
        assert_eq!(draft.pastes, ["large paste"]);
    }
}
