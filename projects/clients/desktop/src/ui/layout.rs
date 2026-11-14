//! The map the window is assembled from: which part stands in which place, in order.
//! Each place draws the parts of its list it knows and passes over the rest, so a list
//! can be reordered or trimmed without code. The defaults are the window as it ships;
//! `gui.json` may hold a `layout` of its own, which is how a person reshapes the window.

use serde::{Deserialize, Serialize};

/// One part of the window that a place can hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    // The rail.
    NewChat,
    Search,
    Chats,
    Schedules,
    Artifacts,
    Connectors,
    Terminal,
    /// A new chat that is kept in memory only.
    Temporary,
    Quiet,
    Settings,
    /// A short rule between groups.
    Divider,
    /// Room that pushes what follows to the far end.
    Spacer,
    // The header's tools.
    Review,
    More,
    /// The iOS Simulator, on macOS.
    Simulator,
    SidePanel,
    /// Plugins' pinned parts of the place it stands in; in the header also the button of
    /// the menu of all their parts.
    Plugins,
    // Around the message box.
    Attach,
    Access,
    Microphone,
    Send,
    Mode,
    Model,
    History,
    Agents,
    Context,
}

/// Where each part stands. A part a place does not know is passed over there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The rail, top to bottom; a `Spacer` sends the rest to its foot.
    pub rail: Vec<Part>,
    /// The header's tools at its right, left to right.
    pub header: Vec<Part>,
    /// Inside the message box, along its foot: the left group, then the right one.
    pub box_left: Vec<Part>,
    pub box_right: Vec<Part>,
    /// Under the box, at its left and right ends.
    pub under_left: Vec<Part>,
    pub under_right: Vec<Part>,
    /// The columns beside the box.
    pub side_left: Vec<Part>,
    pub side_right: Vec<Part>,
}

impl Default for Layout {
    fn default() -> Self {
        use Part::*;
        Self {
            rail: vec![
                NewChat, Search, Divider, Chats, Schedules, Connectors, Artifacts, Plugins, Spacer,
                Temporary, Terminal, Quiet, Settings,
            ],
            header: vec![Review, Search, More, Plugins, Simulator, SidePanel],
            box_left: vec![Attach, Access],
            box_right: vec![Microphone, Send],
            under_left: vec![Mode],
            under_right: vec![Model],
            side_left: vec![History, Plugins],
            side_right: vec![Agents, Context],
        }
    }
}

impl Layout {
    /// A kept rail, header or side of the box that is the order shipped before takes
    /// today's order: `gui.json` holds the whole map, so a rail nobody reshaped would
    /// otherwise never change.
    pub fn refreshed(mut self) -> Self {
        use Part::*;
        let before: [&[Part]; 4] = [
            &[
                NewChat, Search, Divider, Chats, Schedules, Connectors, Artifacts, Spacer,
                Temporary, Terminal, Quiet, Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Connectors, Artifacts, Spacer,
                Terminal, Quiet, Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Artifacts, Connectors, Spacer, Quiet,
                Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Connectors, Artifacts, Spacer, Quiet,
                Settings,
            ],
        ];
        if before.contains(&self.rail.as_slice()) {
            self.rail = Self::default().rail;
        }
        let headers: [&[Part]; 2] = [
            &[Review, Search, More, SidePanel],
            &[Review, Search, More, Simulator, SidePanel],
        ];
        if headers.contains(&self.header.as_slice()) {
            self.header = Self::default().header;
        }
        if self.side_left == [History] {
            self.side_left = Self::default().side_left;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layout_reads_back_and_missing_places_take_the_defaults() {
        let layout: Layout =
            serde_json::from_str(r#"{ "rail": ["settings", "spacer", "chats"] }"#).unwrap();
        assert_eq!(layout.rail, vec![Part::Settings, Part::Spacer, Part::Chats]);
        assert_eq!(layout.header, Layout::default().header);
        assert!(serde_json::from_str::<Layout>(r#"{ "rail": ["nonsense"] }"#).is_err());
    }

    #[test]
    fn a_header_shipped_before_takes_the_plugins_button() {
        use Part::*;
        let old = Layout {
            header: vec![Review, Search, More, Simulator, SidePanel],
            side_left: vec![History],
            ..Layout::default()
        };
        let refreshed = old.refreshed();
        assert!(refreshed.header.contains(&Plugins));
        assert!(refreshed.side_left.contains(&Plugins));
        let own = Layout {
            header: vec![Search],
            ..Layout::default()
        };
        assert_eq!(own.refreshed().header, vec![Search]);
    }
}
