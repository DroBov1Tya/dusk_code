use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Line;

pub const MIN_LAYERS: usize = 2;
pub const MAX_LAYERS: usize = 8;
pub const MAX_CARDS: usize = 8;
pub const LAYER_TITLE_CHARS: usize = 60;
pub const LAYER_SUBTITLE_CHARS: usize = 120;
pub const CARD_TITLE_CHARS: usize = 40;
pub const CARD_LINE_CHARS: usize = 80;

// Plain comments on the types here: a doc comment would go into the schema of `show`
// and so into every request.

// A stack of layers, top to bottom, each joined to the next by an arrow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Flow {
    #[schemars(length(min = MIN_LAYERS, max = MAX_LAYERS))]
    pub layers: Vec<Layer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Layer {
    pub title: Line<LAYER_TITLE_CHARS>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<Line<LAYER_SUBTITLE_CHARS>>,
    // The parts of the layer, drawn as a grid inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(length(max = MAX_CARDS))]
    pub cards: Vec<FlowCard>,
    /// Highlights the layer; at most one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub emphasis: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCard {
    pub title: Line<CARD_TITLE_CHARS>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Line<CARD_LINE_CHARS>>,
}

impl Flow {
    pub(super) fn check(&self) -> anyhow::Result<()> {
        ensure!(
            (MIN_LAYERS..=MAX_LAYERS).contains(&self.layers.len()),
            "a flow has {MIN_LAYERS} to {MAX_LAYERS} layers"
        );
        for (index, layer) in self.layers.iter().enumerate() {
            ensure!(
                layer.cards.len() <= MAX_CARDS,
                "layer {} has over {MAX_CARDS} cards",
                index + 1
            );
        }
        let emphasised = self.layers.iter().filter(|layer| layer.emphasis).count();
        ensure!(emphasised <= 1, "only one layer may have emphasis");
        Ok(())
    }

    pub(super) fn write(&self, text: &mut String) {
        for (index, layer) in self.layers.iter().enumerate() {
            if index > 0 {
                text.push_str("  ↓\n");
            }
            text.push_str(layer.title.as_str());
            if layer.emphasis {
                text.push_str(" (emphasised)");
            }
            if let Some(subtitle) = &layer.subtitle {
                text.push_str(&format!(": {subtitle}"));
            }
            text.push('\n');
            for card in &layer.cards {
                match &card.line {
                    Some(line) => text.push_str(&format!("  - {}: {line}\n", card.title)),
                    None => text.push_str(&format!("  - {}\n", card.title)),
                }
            }
        }
    }
}
