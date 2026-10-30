//! Taking the terminal over and giving it back. Mouse reporting is on for scrolling and folding;
//! most terminals still select text with Shift held. Where the terminal speaks the kitty
//! keyboard protocol, keys come with all their modifiers, so Shift+Enter is not Enter.

use std::{
    env,
    io::{Write, stdout},
};

use anyhow::Result;
use crossterm::{
    event::{
        DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{SetTitle, supports_keyboard_enhancement},
};
use ratatui::DefaultTerminal;

pub fn enter() -> Result<DefaultTerminal> {
    let terminal = ratatui::init();
    // Asked before anything reads stdin; the query restores the raw mode it found.
    super::graphics::detect();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    if supports_keyboard_enhancement().unwrap_or(false) {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    Ok(terminal)
}

pub fn leave() {
    // Restoring a terminal that is already gone has nothing left to report.
    let _ = execute!(
        stdout(),
        PopKeyboardEnhancementFlags,
        DisableMouseCapture,
        DisableBracketedPaste
    );
    ratatui::restore();
}

/// Names the terminal tab. The text may come from the model, so control characters are
/// dropped and cannot reach the terminal as sequences.
pub fn set_title(text: &str) {
    let clean: String = text.chars().filter(|c| !c.is_control()).collect();
    // A title that cannot be set leaves the tab as it was.
    let _ = execute!(stdout(), SetTitle(clean));
}

/// Fixed states only: untrusted transcript text never becomes a terminal payload.
#[derive(Clone, Copy, PartialEq, Eq)]
enum SessionStatus {
    Working,
    Idle,
}

/// iTerm2 Session Status (OSC 21337). Other terminals retain the title fallback.
/// The guard clears only fields owned here, including when the event loop exits early.
pub struct TerminalStatus {
    enabled: bool,
    shown: Option<SessionStatus>,
}

impl TerminalStatus {
    pub fn new() -> Self {
        let enabled = env::var("TERM_PROGRAM").is_ok_and(|value| value == "iTerm.app")
            || env::var("ITERM_SESSION_ID").is_ok_and(|value| !value.is_empty());
        Self {
            enabled,
            shown: None,
        }
    }

    pub fn update(&mut self, working: bool) {
        let status = if working {
            SessionStatus::Working
        } else {
            SessionStatus::Idle
        };
        if !self.enabled || self.shown == Some(status) {
            return;
        }
        let payload = match status {
            SessionStatus::Working => b"\x1b]21337;status=Working;indicator=#ffb454\x07".as_slice(),
            SessionStatus::Idle => b"\x1b]21337;status=Idle;indicator=#5ed6c2\x07".as_slice(),
        };
        if write_status(payload) {
            self.shown = Some(status);
        }
    }
}

impl Drop for TerminalStatus {
    fn drop(&mut self) {
        if self.enabled && self.shown.is_some() {
            write_status(b"\x1b]21337;status=;indicator=\x07");
        }
    }
}

fn write_status(payload: &[u8]) -> bool {
    let mut output = stdout().lock();
    output
        .write_all(payload)
        .and_then(|()| output.flush())
        .is_ok()
}
