//! The `@` names a message may carry: the menu over the box, and completing the word.

use crate::ui::Type;
use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

/// The profile and snippets an `@` word being typed may name; a click puts one in.
pub fn mention_menu(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let names = ws.mention_matches(cx);
    if names.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let mut menu = div()
        .absolute()
        .bottom(relative(1.))
        .left(px(48.))
        .w(px(240.))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg();
    for (index, name) in names.into_iter().enumerate() {
        let label = format!("@{name}");
        menu = menu.child(
            crate::ui::widgets::row(("mention", index), index == 0, cx)
                .small()
                .child(label)
                .on_click(cx.listener(move |ws, _, _, cx| ws.complete_mention(name.clone(), cx))),
        );
    }
    Some(menu)
}

impl Workspace {
    /// Where the `@` word before the cursor starts, and what follows its `@`.
    fn mention_word(&self, cx: &App) -> Option<(usize, String)> {
        let composer = self.composer.read(cx);
        let text = composer.value();
        let before = text.get(..composer.cursor())?;
        let start = before.rfind(char::is_whitespace).map_or(0, |at| {
            at + before[at..].chars().next().map_or(1, char::len_utf8)
        });
        let typed = before[start..].strip_prefix('@')?;
        typed
            .chars()
            .all(dusk_protocol::config::is_name_char)
            .then(|| (start, typed.to_owned()))
    }

    /// What the `@` word being typed may go on to, `me` first, at most eight.
    pub fn mention_matches(&self, cx: &App) -> Vec<String> {
        let Some((_, typed)) = self.mention_word(cx) else {
            return Vec::new();
        };
        self.me
            .names()
            .filter(|name| name.starts_with(typed.as_str()) && *name != typed)
            .take(8)
            .map(str::to_owned)
            .collect()
    }

    /// Puts `@name ` in place of the `@` word being typed.
    pub fn complete_mention(&mut self, name: String, cx: &mut Context<Self>) {
        let Some((start, _)) = self.mention_word(cx) else {
            return;
        };
        let Some(window) = cx.active_window() else {
            return;
        };
        let composer = self.composer.clone();
        let _ = window.update(cx, |_, window, cx| {
            composer.update(cx, |composer, cx| {
                let cursor = composer.cursor();
                composer.set_selected_range(start..cursor, cx);
                composer.replace(format!("@{name} "), window, cx);
            });
        });
        cx.notify();
    }
}
