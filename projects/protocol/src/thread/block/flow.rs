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

// A stack of layers, top to bottom, each joined to the next by an arrow. Built and read
// only through `new`, so one that exists is within its caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "FlowFields")]
pub struct Flow {
    layers: Vec<Layer>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct FlowFields {
    #[schemars(length(min = MIN_LAYERS, max = MAX_LAYERS))]
    layers: Vec<Layer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "LayerFields")]
pub struct Layer {
    pub title: Line<LAYER_TITLE_CHARS>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<Line<LAYER_SUBTITLE_CHARS>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    cards: Vec<FlowCard>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub emphasis: bool,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LayerFields {
    title: Line<LAYER_TITLE_CHARS>,
    subtitle: Option<Line<LAYER_SUBTITLE_CHARS>>,
    // The parts of the layer, drawn as a grid inside it.
    #[serde(default)]
    #[schemars(length(max = MAX_CARDS))]
    cards: Vec<FlowCard>,
    /// Highlights the layer; at most one.
    #[serde(default)]
    emphasis: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FlowCard {
    pub title: Line<CARD_TITLE_CHARS>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<Line<CARD_LINE_CHARS>>,
}

impl Flow {
    pub fn new(layers: Vec<Layer>) -> anyhow::Result<Self> {
        ensure!(
            (MIN_LAYERS..=MAX_LAYERS).contains(&layers.len()),
            "a flow has {MIN_LAYERS} to {MAX_LAYERS} layers"
        );
        let emphasised = layers.iter().filter(|layer| layer.emphasis).count();
        ensure!(emphasised <= 1, "only one layer may have emphasis");
        Ok(Self { layers })
    }

    pub fn layers(&self) -> &[Layer] {
        &self.layers
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

impl Layer {
    pub fn new(
        title: Line<LAYER_TITLE_CHARS>,
        subtitle: Option<Line<LAYER_SUBTITLE_CHARS>>,
        cards: Vec<FlowCard>,
        emphasis: bool,
    ) -> anyhow::Result<Self> {
        ensure!(
            cards.len() <= MAX_CARDS,
            "the layer {:?} has over {MAX_CARDS} cards",
            title.as_str()
        );
        Ok(Self {
            title,
            subtitle,
            cards,
            emphasis,
        })
    }

    pub fn cards(&self) -> &[FlowCard] {
        &self.cards
    }
}

impl TryFrom<FlowFields> for Flow {
    type Error = anyhow::Error;

    fn try_from(fields: FlowFields) -> anyhow::Result<Self> {
        Self::new(fields.layers)
    }
}

impl TryFrom<LayerFields> for Layer {
    type Error = anyhow::Error;

    fn try_from(fields: LayerFields) -> anyhow::Result<Self> {
        Self::new(fields.title, fields.subtitle, fields.cards, fields.emphasis)
    }
}
