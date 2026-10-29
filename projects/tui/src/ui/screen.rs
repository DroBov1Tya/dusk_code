//! Taking the terminal over and giving it back. Mouse reporting is on for scrolling and folding;
//! most terminals still select text with Shift held.

use std::io::stdout;

use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
};
use ratatui::DefaultTerminal;

pub fn enter() -> Result<DefaultTerminal> {
    let terminal = ratatui::init();
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste)?;
    Ok(terminal)
}

pub fn leave() {
    // Restoring a terminal that is already gone has nothing left to report.
    let _ = execute!(stdout(), DisableMouseCapture, DisableBracketedPaste);
    ratatui::restore();
}
