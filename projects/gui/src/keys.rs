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
        ToggleTerminal,
        SaveFile,
        Quit,
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
    ("secondary-j", "Show or hide the terminal"),
    ("secondary-s", "Save the open file"),
    ("secondary-q", "Quit"),
    (
        "escape",
        "Close the menu or window; leave the review or the box",
    ),
];

pub fn bind(cx: &mut App) {
    // The parser takes names that live for the whole run, so each platform's are literals.
    let (terminal, save) = if cfg!(target_os = "macos") {
        ("cmd-j", "cmd-s")
    } else {
        // Control-J and Control-S are the shell's own keys, so these take Shift too.
        ("ctrl-shift-j", "ctrl-shift-s")
    };
    let (new, find, settings, sidebar, side, quit) = if cfg!(target_os = "macos") {
        ("cmd-n", "cmd-k", "cmd-,", "cmd-b", "cmd-alt-b", "cmd-q")
    } else {
        (
            "ctrl-n",
            "ctrl-k",
            "ctrl-,",
            "ctrl-b",
            "ctrl-alt-b",
            "ctrl-q",
        )
    };
    cx.bind_keys([
        KeyBinding::new(new, NewChat, None),
        KeyBinding::new(find, FindChat, None),
        KeyBinding::new(settings, OpenSettings, None),
        KeyBinding::new(sidebar, ToggleSidebar, None),
        KeyBinding::new(side, ToggleSidePanel, None),
        KeyBinding::new(terminal, ToggleTerminal, None),
        KeyBinding::new(save, SaveFile, None),
        KeyBinding::new(quit, Quit, None),
        // Tab walks the rows only while a row has the keyboard; in a box it stays the box's.
        KeyBinding::new("tab", FocusNextRow, Some("Row")),
        KeyBinding::new("shift-tab", FocusPrevRow, Some("Row")),
        KeyBinding::new("tab", FocusNextRow, Some("Workspace")),
        KeyBinding::new("shift-tab", FocusPrevRow, Some("Workspace")),
    ]);
    // Quit works wherever the focus is, and the Mac's application menu shows it.
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus(vec![Menu {
        name: "Duskcode".into(),
        items: vec![MenuItem::action("Quit Duskcode", Quit)],
        disabled: false,
    }]);
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

    pub fn on_toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_terminal(window, cx);
    }

    pub fn on_save_file(&mut self, _: &SaveFile, _: &mut Window, cx: &mut Context<Self>) {
        self.save_file(cx);
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
    pub fn close_top(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.modal.is_some() {
            self.close_modal_from_window(window, cx);
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
    let rows = SHORTCUTS.iter().map(|(binding, what)| {
        crate::pages::settings::line(what, "", cx).child(
            div()
                .px_2()
                .py_0p5()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .bg(raised)
                .text_color(muted)
                .child(show(binding)),
        )
    });
    let body = div()
        .flex()
        .flex_col()
        .gap_3()
        .child(crate::pages::settings::list(rows.collect::<Vec<_>>(), cx));
    body.child(crate::ui::widgets::muted(
        "Escape leaves the message box; then Tab moves through the rows, and Enter or Space presses the one in focus.",
        cx,
    ))
}
