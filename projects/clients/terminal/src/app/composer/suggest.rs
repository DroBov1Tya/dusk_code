//! The user's likely next message, as the model proposed it (`suggest_next`): greyed in
//! the empty box in place of its hint. Tab takes it in; Esc puts it away.

use super::App;

impl App {
    /// The suggestion shown in the box: only while the box is empty.
    pub fn suggestion_ghost(&self) -> Option<&str> {
        self.input
            .is_empty()
            .then(|| self.suggestion.ghost(""))
            .flatten()
    }

    /// Tab over the empty box with a suggestion: it becomes the text, the cursor at its
    /// end. False leaves Tab its own meaning.
    pub(crate) fn accept_suggestion(&mut self) -> bool {
        let Some(text) = self.suggestion_ghost().map(str::to_owned) else {
            return false;
        };
        self.input.set(text);
        true
    }
}

#[cfg(test)]
mod tests {
    use crate::app::{Action, tabs::tests::app};
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use dusk_protocol::thread::ThreadEvent;

    fn press(app: &mut crate::app::App, code: KeyCode) -> Action {
        app.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn tab_takes_the_suggestion_into_the_empty_box_only() {
        let mut app = app();
        // With nothing suggested Tab switches the mode, as before.
        assert!(matches!(press(&mut app, KeyCode::Tab), Action::NextMode));
        app.apply(ThreadEvent::NextSuggested {
            text: "run the tests".into(),
        });
        assert_eq!(app.suggestion_ghost(), Some("run the tests"));
        // Typing hides it, and Tab over text keeps its meaning.
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.suggestion_ghost(), None);
        assert!(matches!(press(&mut app, KeyCode::Tab), Action::NextMode));
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.suggestion_ghost(), Some("run the tests"));
        assert!(matches!(press(&mut app, KeyCode::Tab), Action::None));
        assert_eq!(app.input.text, "run the tests");
        assert_eq!(app.input.cursor, app.input.text.len());
        // Sending it makes it stale.
        app.submit(false);
        app.input.take();
        assert_eq!(app.suggestion_ghost(), None);
    }

    #[test]
    fn esc_puts_it_away_until_the_next_one() {
        let mut app = app();
        app.apply(ThreadEvent::NextSuggested {
            text: "commit it".into(),
        });
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.suggestion_ghost(), None);
        assert!(matches!(press(&mut app, KeyCode::Tab), Action::NextMode));
        app.apply(ThreadEvent::NextSuggested {
            text: "push it".into(),
        });
        assert_eq!(app.suggestion_ghost(), Some("push it"));
    }
}
