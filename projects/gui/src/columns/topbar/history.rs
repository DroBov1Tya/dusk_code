//! Back and forward through the places the user went: the page, the chat shown, the
//! settings page. A place is recorded when it changes, whatever changed it.

use crate::{
    pages::settings::SettingsPage,
    workspace::{Center, Workspace},
};

/// How many places back are kept.
const DEPTH: usize = 50;

#[derive(Clone, PartialEq)]
pub struct Place {
    center: Center,
    shown: Option<String>,
    settings: SettingsPage,
}

#[derive(Default)]
pub struct History {
    back: Vec<Place>,
    forward: Vec<Place>,
    current: Option<Place>,
}

impl History {
    pub fn can_go_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Notes where the window is; a new place ends the way forward.
    fn record(&mut self, place: Place) {
        match self.current.replace(place.clone()) {
            Some(previous) if previous != place => {
                self.back.push(previous);
                if self.back.len() > DEPTH {
                    self.back.remove(0);
                }
                self.forward.clear();
            }
            _ => {}
        }
    }

    fn step(&mut self, back: bool) -> Option<Place> {
        let (from, to) = if back {
            (&mut self.back, &mut self.forward)
        } else {
            (&mut self.forward, &mut self.back)
        };
        let place = from.pop()?;
        if let Some(current) = self.current.replace(place.clone()) {
            to.push(current);
        }
        Some(place)
    }
}

impl Workspace {
    fn place(&self) -> Place {
        Place {
            center: self.center,
            shown: self.shown.clone(),
            settings: self.settings_page,
        }
    }

    /// Called on every draw, so every way of moving is caught in one place.
    pub fn record_place(&mut self) {
        let place = self.place();
        self.history.record(place);
    }

    pub fn go(&mut self, back: bool, cx: &mut gpui_kit::Context<Self>) {
        let Some(place) = self.history.step(back) else {
            return;
        };
        self.settings_page = place.settings;
        if place.center == Center::Settings {
            self.load_page(place.settings, cx);
        }
        if place.shown != self.shown {
            match &place.shown {
                Some(thread_id) => self.open_chat(thread_id, cx),
                None => self.shown = None,
            }
        }
        self.center = place.center;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(center: Center) -> Place {
        Place {
            center,
            shown: None,
            settings: SettingsPage::Appearance,
        }
    }

    #[test]
    fn back_and_forward_walk_the_places_and_a_new_one_ends_the_way_forward() {
        let mut history = History::default();
        for center in [Center::Chat, Center::Settings, Center::Artifacts] {
            history.record(at(center));
        }
        assert!(history.step(true) == Some(at(Center::Settings)));
        assert!(history.step(true) == Some(at(Center::Chat)));
        assert!(history.step(true).is_none());
        assert!(history.step(false) == Some(at(Center::Settings)));
        history.record(at(Center::Schedules));
        assert!(!history.can_go_forward());
        assert!(history.step(true) == Some(at(Center::Settings)));
    }
}
