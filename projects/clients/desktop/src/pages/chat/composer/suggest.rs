//! The user's likely next message, as the model proposed it (`suggest_next`): greyed in
//! the empty box, where the placeholder sits, so typing hides it and emptying the box
//! shows it again. Tab takes it in with the caret at its end; Esc puts it away until the
//! next one. Each chat keeps its own, as `Suggestion` follows the chat's events.

use dusk_protocol::thread::Suggestion;
use gpui_kit::{component::ActiveTheme, *};

use crate::{ui::Type, workspace::Workspace};

/// What the empty box says when no suggestion is there.
pub const PLACEHOLDER: &str = "Ask anything. Enter sends, Shift+Enter starts a new line.";

/// The placeholder the box shows: the suggestion while there is one to show.
fn placeholder(suggestion: Option<&Suggestion>) -> &str {
    suggestion
        .and_then(|suggestion| suggestion.ghost(""))
        .unwrap_or(PLACEHOLDER)
}

impl Workspace {
    fn suggestion(&self) -> Option<&Suggestion> {
        self.suggestions.get(self.shown.as_deref()?)
    }

    /// The suggestion shown over the box's text as it is now.
    pub fn suggestion_ghost(&self, cx: &App) -> Option<&str> {
        let input = self.composer.read(cx).value();
        self.suggestion()?.ghost(&input)
    }

    /// Tab over the empty box with a suggestion in it: the suggestion becomes the text.
    /// False leaves Tab its own meaning.
    pub fn accept_suggestion(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let input = self.composer.read(cx).value();
        let Some(text) = self
            .suggestion()
            .and_then(|suggestion| suggestion.accept(&input))
        else {
            return false;
        };
        self.composer.update(cx, |composer, cx| {
            composer.replace(text, window, cx);
        });
        cx.notify();
        true
    }

    /// Esc in the box while a suggestion shows there: hidden until the next one.
    pub fn dismiss_suggestion(&mut self, window: &Window, cx: &mut Context<Self>) -> bool {
        let writing = Focusable::focus_handle(self.composer.read(cx), cx).is_focused(window);
        if !writing || self.suggestion_ghost(cx).is_none() {
            return false;
        }
        let shown = self.shown.clone().unwrap_or_default();
        self.suggestions
            .get_mut(&shown)
            .is_some_and(Suggestion::dismiss)
    }

    /// The user sent a message to `thread_id`: what was suggested is stale.
    pub fn drop_suggestion(&mut self, thread_id: Option<&str>) {
        if let Some(suggestion) = thread_id.and_then(|id| self.suggestions.get_mut(id)) {
            suggestion.clear();
        }
    }
}

/// Puts the shown chat's suggestion where the placeholder is, or the placeholder back.
/// Called as the chat page is drawn, beside `show_draft`.
pub fn show_suggestion(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let wanted = placeholder(ws.suggestion()).to_owned();
    if ws.placeholder_shown == wanted {
        return;
    }
    ws.placeholder_shown = wanted.clone();
    ws.composer.update(cx, |composer, cx| {
        composer.set_placeholder(wanted, window, cx);
    });
}

/// The key that takes the suggestion in, at the right end of its line.
pub fn tab_hint(ws: &Workspace, cx: &App) -> Option<Div> {
    ws.suggestion_ghost(cx)?;
    Some(
        div()
            .absolute()
            .top(px(6.))
            .right(px(10.))
            .px_1()
            .rounded_sm()
            .border_1()
            .border_color(cx.theme().border)
            .meta()
            .text_color(cx.theme().muted_foreground)
            .child("Tab"),
    )
}

#[cfg(test)]
mod tests {
    use dusk_protocol::thread::ThreadEvent;

    use super::{PLACEHOLDER, Suggestion, placeholder};

    #[test]
    fn the_box_shows_the_suggestion_until_it_is_stale_or_put_away() {
        let mut suggestion = Suggestion::default();
        assert_eq!(placeholder(None), PLACEHOLDER);
        assert_eq!(placeholder(Some(&suggestion)), PLACEHOLDER);
        suggestion.hear(&ThreadEvent::NextSuggested {
            text: "Да, встраивай все".into(),
        });
        assert_eq!(placeholder(Some(&suggestion)), "Да, встраивай все");
        // Tab takes it only over an empty box; over text Tab keeps its meaning.
        assert_eq!(suggestion.accept("Д"), None);
        assert_eq!(suggestion.accept("").as_deref(), Some("Да, встраивай все"));
        assert!(suggestion.dismiss());
        assert_eq!(placeholder(Some(&suggestion)), PLACEHOLDER);
        suggestion.hear(&ThreadEvent::NextSuggested {
            text: "run the tests".into(),
        });
        suggestion.hear(&ThreadEvent::Queued {
            message_id: "m".into(),
        });
        assert_eq!(placeholder(Some(&suggestion)), PLACEHOLDER);
    }
}
