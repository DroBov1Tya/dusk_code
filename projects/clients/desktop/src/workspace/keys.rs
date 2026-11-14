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
    ("secondary-k", "Find a chat, or what was said in one"),
    ("secondary-f", "The same search"),
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
    let (terminal, save, find_text) = if cfg!(target_os = "macos") {
        ("cmd-j", "cmd-s", "cmd-f")
    } else {
        // Control-J, Control-S and Control-F are the shell's own keys, so these take
        // Shift too.
        ("ctrl-shift-j", "ctrl-shift-s", "ctrl-shift-f")
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
        KeyBinding::new(find_text, FindChat, None),
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
        if std::mem::take(&mut self.path_menu)
            || std::mem::take(&mut self.attach_open)
            || std::mem::take(&mut self.plugins.menu)
        {
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
