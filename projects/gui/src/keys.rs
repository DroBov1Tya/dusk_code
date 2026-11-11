//! The keyboard: shortcuts for the places people go to most, and what Escape closes. A
//! shortcut uses the platform's main key (Command on a Mac, Control elsewhere), so it never
//! collides with typing; Escape closes the topmost thing that is open, one at a time.

use gpui_kit::*;

use crate::workspace::{Center, Workspace};

actions!(
    duskcode,
    [
        NewChat,
        FindChat,
        OpenSettings,
        ToggleSidebar,
        ToggleSidePanel,
        FocusNextRow,
        FocusPrevRow
    ]
);

/// The shortcuts and what they do, in the order the help lists them.
pub const SHORTCUTS: &[(&str, &str)] = &[
    ("secondary-n", "New chat"),
    ("secondary-k", "Find a chat"),
    ("secondary-,", "Settings"),
    ("secondary-b", "Show or hide the sidebar"),
    ("secondary-alt-b", "Show or hide the side panel"),
    (
        "escape",
        "Close the menu or window; leave the review or the box",
    ),
];

pub fn bind(cx: &mut App) {
    // The parser takes names that live for the whole run, so each platform's are literals.
    let (new, find, settings, sidebar, side) = if cfg!(target_os = "macos") {
        ("cmd-n", "cmd-k", "cmd-,", "cmd-b", "cmd-alt-b")
    } else {
        ("ctrl-n", "ctrl-k", "ctrl-,", "ctrl-b", "ctrl-alt-b")
    };
    cx.bind_keys([
        KeyBinding::new(new, NewChat, None),
        KeyBinding::new(find, FindChat, None),
        KeyBinding::new(settings, OpenSettings, None),
        KeyBinding::new(sidebar, ToggleSidebar, None),
        KeyBinding::new(side, ToggleSidePanel, None),
        // Tab walks the rows only while a row has the keyboard; in a box it stays the box's.
        KeyBinding::new("tab", FocusNextRow, Some("Row")),
        KeyBinding::new("shift-tab", FocusPrevRow, Some("Row")),
        KeyBinding::new("tab", FocusNextRow, Some("Workspace")),
        KeyBinding::new("shift-tab", FocusPrevRow, Some("Workspace")),
    ]);
}

impl Workspace {
    pub fn on_focus_next(&mut self, _: &FocusNextRow, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_next(cx);
    }

    pub fn on_focus_prev(&mut self, _: &FocusPrevRow, window: &mut Window, cx: &mut Context<Self>) {
        window.focus_prev(cx);
    }

    pub fn on_new_chat(&mut self, _: &NewChat, window: &mut Window, cx: &mut Context<Self>) {
        self.new_chat(None, window, cx);
    }

    pub fn on_find_chat(&mut self, _: &FindChat, _: &mut Window, cx: &mut Context<Self>) {
        self.modal = Some(crate::modal::Modal::Search);
        cx.notify();
    }

    pub fn on_open_settings(&mut self, _: &OpenSettings, _: &mut Window, cx: &mut Context<Self>) {
        self.center = Center::Settings;
        self.load_settings(cx);
    }

    pub fn on_toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.left_hidden = !self.left_hidden;
        cx.notify();
    }

    pub fn on_toggle_side_panel(
        &mut self,
        _: &ToggleSidePanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.right_hidden = !self.right_hidden;
        cx.notify();
    }

    /// Escape closes what is on top, one thing a press: a window, a menu, then the review.
    /// True when something closed, so the key goes no further.
    pub fn close_top(&mut self) -> bool {
        if self.modal.take().is_some() {
            return true;
        }
        if self.chat_menu.take().is_some()
            || self.artifact_menu.take().is_some()
            || self.audio.menu.take().is_some()
        {
            return true;
        }
        if std::mem::take(&mut self.path_menu) || std::mem::take(&mut self.attach_open) {
            return true;
        }
        if std::mem::take(&mut self.navigator_open) {
            return true;
        }
        if self.center == Center::Review {
            self.center = Center::Chat;
            return true;
        }
        false
    }
}

/// How a binding reads on this platform: `secondary-alt-b` is ⌘⌥B on a Mac, Ctrl+Alt+B elsewhere.
fn show(binding: &str) -> String {
    let mac = cfg!(target_os = "macos");
    binding
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| match (part, mac) {
            ("secondary", true) => "\u{2318}".to_owned(),
            ("secondary", false) => "Ctrl".to_owned(),
            ("alt", true) => "\u{2325}".to_owned(),
            ("alt", false) => "Alt".to_owned(),
            ("escape", _) => "Esc".to_owned(),
            (other, _) => other.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join(if mac { "" } else { "+" })
}

/// The settings page that lists the shortcuts.
pub fn page(cx: &mut Context<Workspace>) -> Div {
    use gpui_kit::component::ActiveTheme;
    let (muted, raised) = (cx.theme().muted_foreground, cx.theme().accent);
    let mut body = div().flex().flex_col();
    for (binding, what) in SHORTCUTS {
        body = body.child(
            crate::settings::line(what, "", cx).child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .bg(raised)
                    .text_color(muted)
                    .child(show(binding)),
            ),
        );
    }
    body.child(crate::widgets::muted(
        "Escape leaves the message box; then Tab moves through the rows, and Enter or Space presses the one in focus.",
        cx,
    ))
}
