//! Taking the terminal over and giving it back. Mouse reporting is on for scrolling and folding;
//! most terminals still select text with Shift held. Where the terminal speaks the kitty
//! keyboard protocol, keys come with all their modifiers, so Shift+Enter is not Enter.

use std::io::stdout;

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
