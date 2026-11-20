//! Keys of the Workshop. Letters narrow the list, so its own keys are arrows, Enter,
//! Tab, Esc and Ctrl; a pack's page has no filter, so there `i` installs.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use dusk_protocol::{client::Switch, plugin::Kind};

use crate::app::{
    Action, App, Panel,
    panels::{Choice, Confirm, PackPage, Row, Workshop, choices, step},
};

impl App {
    pub(super) fn workshop_key(&mut self, key: KeyEvent) -> Action {
        let Some(Panel::Workshop(view)) = &mut self.panel else {
            return Action::None;
        };
        if view.confirm.is_some() {
            return confirm_key(view, key);
        }
        if view.pack.is_some() {
            return self.pack_key(key);
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('r') if ctrl => return Action::LoadStore,
            KeyCode::Char('u') if ctrl => return Action::UpdateAll,
            KeyCode::Char('o') if ctrl => {
                view.next_order();
                return Action::LoadStore;
            }
            KeyCode::Tab => {
                view.next_category();
                return Action::LoadStore;
            }
            KeyCode::Up | KeyCode::Down => {
                let count = view.shown().len();
                step(&mut view.selected, count, key.code == KeyCode::Down);
            }
            KeyCode::Enter => {
                let pack = match view.shown().get(view.selected) {
                    Some(Row::Pack(pack)) => Some(pack.name.clone()),
                    Some(Row::Listing(listing)) if listing.kind == Kind::Pack => {
                        Some(listing.name.clone())
                    }
                    Some(Row::Listing(listing)) => {
                        let listing = (*listing).clone();
                        return self.ask(listing);
                    }
                    None => None,
                };
                if let Some(name) = pack {
                    view.pack = Some(PackPage { name, selected: 0 });
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

    /// Opens the box for `listing`, or says why there is nothing to do.
    fn ask(&mut self, listing: dusk_protocol::plugin::Listing) -> Action {
        let Some(Panel::Workshop(view)) = &mut self.panel else {
            return Action::None;
        };
        if choices(&listing) == [Choice::Cancel] {
            let why = listing.unfit.as_deref().unwrap_or("already installed");
            self.flash = Some(format!("cannot install here: {why}"));
        } else {
            let unverified = crate::app::reviewed_first(&listing);
            let review = Action::ReviewListing {
                source: listing.source.clone(),
                name: listing.name.clone(),
            };
            view.confirm = Some(Confirm {
                listing,
                focus: 0,
                review: unverified.then_some(crate::app::Reviewed::Reading),
            });
            // Nobody signed its list: its files are read and shown before the yes.
            if unverified {
                return review;
            }
        }
        Action::None
    }

    fn pack_key(&mut self, key: KeyEvent) -> Action {
        let Some(Panel::Workshop(view)) = &mut self.panel else {
            return Action::None;
        };
        let Some(page) = &view.pack else {
            return Action::None;
        };
        let here = view.pack_here(&page.name);
        let rows = here.map_or(0, |pack| pack.skills.len() + 1);
        // Row 0 is the pack's switch, then its skills.
        let switch = here.and_then(|pack| match page.selected {
            0 => Some((Switch::SkillPack(pack.name.clone()), !pack.enabled)),
            row => pack
                .skills
                .get(row - 1)
                .map(|skill| (Switch::Skill(skill.name.clone()), !skill.enabled)),
        });
        let listing = view.pack_listing(&page.name).cloned();
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                if let Some(page) = &mut view.pack {
                    step(&mut page.selected, rows, key.code == KeyCode::Down);
                }
            }
            KeyCode::Enter | KeyCode::Char(' ') => {
                if let Some((switch, on)) = switch {
                    return Action::PackSwitch(switch, on);
                }
            }
            KeyCode::Char('i') => match listing {
                Some(listing) => return self.ask(listing),
                None => self.flash = Some("no source lists this pack".into()),
            },
            KeyCode::Esc => view.pack = None,
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
                Some(Choice::Install | Choice::Update) => match confirm.agreed() {
                    Some(sha256) => Action::InstallPlugin(Box::new(listing.clone()), sha256),
                    // The files are not shown yet, or cannot be installed.
                    None => return Action::None,
                },
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
