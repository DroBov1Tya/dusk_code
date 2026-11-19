//! The map the window is assembled from: which part stands in which place, in order.
//! Each place draws the parts of its list it knows and passes over the rest, so a list
//! can be reordered or trimmed without code. The defaults are the window as it ships;
//! `gui.json` may hold a `layout` of its own, which is how a person reshapes the window.

use serde::{Deserialize, Deserializer, Serialize, de::IntoDeserializer};

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
    #[serde(alias = "connectors")]
    Customize,
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
    /// How full the context is: the ring beside the box.
    Context,
}

/// Where each part stands. A part a place does not know is passed over there.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    /// The rail, top to bottom; a `Spacer` sends the rest to its foot.
    #[serde(deserialize_with = "parts")]
    pub rail: Vec<Part>,
    /// The header's tools at its right, left to right.
    #[serde(deserialize_with = "parts")]
    pub header: Vec<Part>,
    /// Inside the message box, along its foot: the left group, then the right one.
    #[serde(deserialize_with = "parts")]
    pub box_left: Vec<Part>,
    #[serde(deserialize_with = "parts")]
    pub box_right: Vec<Part>,
    /// Under the box, at its left and right ends.
    #[serde(deserialize_with = "parts")]
    pub under_left: Vec<Part>,
    #[serde(deserialize_with = "parts")]
    pub under_right: Vec<Part>,
    /// The columns beside the box.
    #[serde(deserialize_with = "parts")]
    pub side_left: Vec<Part>,
    #[serde(deserialize_with = "parts")]
    pub side_right: Vec<Part>,
}

/// Parts an earlier version had; a kept layout that names one loads without it.
const RETIRED: &[&str] = &["simulator"];

/// A list of parts, less the retired ones. Any other name it does not know is an error.
fn parts<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<Part>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .filter(|name| !RETIRED.contains(&name.as_str()))
        .map(|name| Part::deserialize(IntoDeserializer::<D::Error>::into_deserializer(name)))
        .collect()
}

impl Default for Layout {
    fn default() -> Self {
        use Part::*;
        Self {
            rail: vec![
                NewChat, Search, Divider, Chats, Schedules, Customize, Artifacts, Plugins, Spacer,
                Temporary, Terminal, Quiet, Settings,
            ],
            header: vec![Review, Search, More, Plugins, SidePanel],
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
                NewChat, Search, Divider, Chats, Schedules, Customize, Artifacts, Spacer,
                Temporary, Terminal, Quiet, Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Customize, Artifacts, Spacer, Terminal,
                Quiet, Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Artifacts, Customize, Spacer, Quiet,
                Settings,
            ],
            &[
                NewChat, Search, Divider, Chats, Schedules, Customize, Artifacts, Spacer, Quiet,
                Settings,
            ],
        ];
        if before.contains(&self.rail.as_slice()) {
            self.rail = Self::default().rail;
        }
        // The header shipped with the iOS Simulator reads back as this one, without it.
        let headers: [&[Part]; 1] = [&[Review, Search, More, SidePanel]];
        if headers.contains(&self.header.as_slice()) {
            self.header = Self::default().header;
        }
        if self.side_left == [History] {
            self.side_left = Self::default().side_left;
        }
        // For a while the context ring stood in the panel over the box and the side kept
        // the subagents alone; a file kept then takes the ring back.
        if self.side_right == [Agents] {
            self.side_right = Self::default().side_right;
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
    fn a_retired_part_is_dropped_and_the_rest_reads_back() {
        let layout: Layout = serde_json::from_str(
            r#"{ "header": ["review", "search", "more", "plugins", "simulator", "side_panel"] }"#,
        )
        .unwrap();
        assert_eq!(layout.header, Layout::default().header);
    }

    #[test]
    fn the_context_ring_stands_beside_the_box_whichever_file_was_kept() {
        let with: Layout =
            serde_json::from_str(r#"{ "side_right": ["agents", "context"] }"#).unwrap();
        let without: Layout = serde_json::from_str(r#"{ "side_right": ["agents"] }"#).unwrap();
        for kept in [with, without] {
            assert_eq!(kept.refreshed().side_right, [Part::Agents, Part::Context]);
        }
    }

    #[test]
    fn a_header_shipped_before_takes_the_plugins_button() {
        let old: Layout = serde_json::from_str(
            r#"{ "header": ["review", "search", "more", "simulator", "side_panel"],
                 "side_left": ["history"] }"#,
        )
        .unwrap();
        let refreshed = old.refreshed();
        assert!(refreshed.header.contains(&Part::Plugins));
        assert!(refreshed.side_left.contains(&Part::Plugins));
        let own = Layout {
            header: vec![Part::Search],
            ..Layout::default()
        };
        assert_eq!(own.refreshed().header, vec![Part::Search]);
    }

    #[test]
    fn a_rail_kept_with_connectors_opens_customize() {
        let kept: Layout =
            serde_json::from_str(r#"{ "rail": ["chats", "connectors", "settings"] }"#).unwrap();
        assert_eq!(kept.rail, [Part::Chats, Part::Customize, Part::Settings]);
        let written = serde_json::to_string(&kept).unwrap();
        assert!(written.contains(r#""customize""#), "{written}");
        let shipped: Layout = serde_json::from_str(
            r#"{ "rail": ["new_chat", "search", "divider", "chats", "schedules", "connectors",
                          "artifacts", "spacer", "quiet", "settings"] }"#,
        )
        .unwrap();
        assert_eq!(shipped.refreshed().rail, Layout::default().rail);
    }
}
