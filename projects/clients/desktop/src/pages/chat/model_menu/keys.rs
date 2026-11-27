//! Keys while a menu over the model bar is open, and the page's own keys when none is:
//! Escape, the digits that pick a model, Up and Down on the review.

use gpui_kit::*;

use super::FIRST;
use crate::workspace::{Center, Picker, Workspace};

impl Workspace {
    /// The first models of the provider in use, as `provider/model`, each picked by its
    /// digit in More models while that provider is unfolded there.
    pub fn first_models(&self) -> Vec<String> {
        let current = self.chat_model().unwrap_or_default();
        let provider = current.split('/').next().unwrap_or_default();
        let mine = self
            .providers
            .iter()
            .find(|p| p.name == provider)
            .or(self.providers.first());
        let Some((p, Ok(models))) = mine.map(|p| (p, &p.models)) else {
            return Vec::new();
        };
        if !self.models_open.contains(&p.name) {
            return Vec::new();
        }
        models
            .iter()
            .take(FIRST)
            .map(|model| format!("{}/{}", p.name, model.id))
            .collect()
    }

    /// While a menu over the model bar is open, Escape closes it; in More models the
    /// digits 1-6 pick the first models of the provider in use, before the text box
    /// takes them.
    pub fn menu_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // Escape and the arrows are the shell's while it has the keyboard.
        if self.terminal_focused(window) {
            return;
        }
        let Some(picker) = self.picker else {
            self.page_keys(event, window, cx);
            return;
        };
        let keys = &event.keystroke;
        if keys.modifiers.modified() {
            return;
        }
        if keys.key == "escape" {
            cx.stop_propagation();
            self.picker = None;
            cx.notify();
            return;
        }
        let digit = keys
            .key
            .parse::<usize>()
            .ok()
            .filter(|d| (1..=FIRST).contains(d));
        if picker != Picker::MoreModels || self.model_input.focus_handle(cx).is_focused(window) {
            return;
        }
        if let Some(name) = digit.and_then(|digit| self.first_models().get(digit - 1).cloned()) {
            cx.stop_propagation();
            self.choose_model(name, cx);
        }
    }

    /// Keys of the page in the middle: on the review, Up and Down choose the file and
    /// Escape goes back to the chat; on the welcome, Left and Right turn the pages.
    /// Letters are left alone, so typing still reaches the boxes.
    fn page_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keys = &event.keystroke;
        if keys.modifiers.modified() {
            return;
        }
        match keys.key.as_str() {
            "escape" => {
                // Nothing open: Escape leaves the box for the window, from where Tab walks
                // the rows.
                if !self.close_top(window, cx) {
                    if self.focus.is_focused(window) {
                        return;
                    }
                    window.focus(&self.focus, cx);
                }
            }
            // The welcome's pages turn sideways, unless a box has the arrows.
            "left" | "right"
                if crate::pages::welcome::shown(self)
                    && self.modal.is_none()
                    && !self.welcome.typing(window, cx) =>
            {
                self.welcome_turn(if keys.key == "right" { 1 } else { -1 }, window, cx)
            }
            "down" | "up" if self.center == Center::Review && self.modal.is_none() => {
                self.review.step(if keys.key == "down" { 1 } else { -1 })
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
