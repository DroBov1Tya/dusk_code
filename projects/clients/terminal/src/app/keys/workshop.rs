//! Keys of the Workshop. Letters narrow the list, so its own keys are arrows, Enter, Esc
//! and Ctrl+R.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{
    Action, App, Panel,
    panels::{Choice, Confirm, Workshop, choices, step},
};

impl App {
    pub(super) fn workshop_key(&mut self, key: KeyEvent) -> Action {
        let Some(Panel::Workshop(view)) = &mut self.panel else {
            return Action::None;
        };
        if view.confirm.is_some() {
            return confirm_key(view, key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('r') if ctrl => return Action::LoadStore,
            KeyCode::Up | KeyCode::Down => {
                let count = view.shown().len();
                step(&mut view.selected, count, key.code == KeyCode::Down);
            }
            KeyCode::Enter => {
                let Some(listing) = view.shown().get(view.selected).map(|l| (*l).clone()) else {
                    return Action::None;
                };
                if choices(&listing) == [Choice::Cancel] {
                    let why = listing.unfit.as_deref().unwrap_or("already installed");
                    self.flash = Some(format!("cannot install here: {why}"));
                } else {
                    view.confirm = Some(Confirm { listing, focus: 0 });
                }
            }
            KeyCode::Backspace => {
                view.filter.pop();
                view.selected = 0;
            }
            KeyCode::Char(c) if !ctrl && !alt && !c.is_control() => {
                view.filter.push(c);
                view.selected = 0;
            }
            KeyCode::Esc if !view.filter.is_empty() => {
                view.filter.clear();
                view.selected = 0;
            }
            KeyCode::Esc => {
                self.panel = Some(Panel::Plugins {
                    selected: 0,
                    entry: None,
                });
            }
            _ => {}
        }
        Action::None
    }
}

fn confirm_key(view: &mut Workshop, key: KeyEvent) -> Action {
    let Some(confirm) = &mut view.confirm else {
        return Action::None;
    };
    let offered = choices(&confirm.listing);
    let count = offered.len();
    match key.code {
        KeyCode::Esc => view.confirm = None,
        KeyCode::Right | KeyCode::Tab => confirm.focus = (confirm.focus + 1) % count,
        KeyCode::Left | KeyCode::BackTab => confirm.focus = (confirm.focus + count - 1) % count,
        KeyCode::Enter => {
            let listing = &confirm.listing;
            let action = match offered.get(confirm.focus) {
                Some(Choice::Install | Choice::Update) => {
                    Action::InstallPlugin(Box::new(listing.clone()))
                }
                Some(Choice::Remove) => Action::RemovePlugin(listing.name.clone()),
                Some(Choice::Cancel) | None => Action::None,
            };
            view.confirm = None;
            return action;
        }
        _ => {}
    }
    Action::None
}
