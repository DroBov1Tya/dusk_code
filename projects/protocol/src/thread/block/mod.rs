//! Structured content a chat shows that is not markdown: a flow of layers, a list of
//! checks, reply buttons. The model makes a block with one call of the `show` tool; the
//! call's arguments, which the thread stores with the assistant message, are the block.
//! Every text in a block is a `Line`: plain, capped, never read as markdown or a link.

mod checks;
mod choices;
mod flow;
mod line;

pub use checks::{
    CHECK_DETAIL_CHARS, CHECK_NAME_CHARS, Check, CheckState, Checks, Counts, MAX_CHECKS,
};
pub use choices::{CHOICE_CHARS, Choices, MAX_CHOICES, MIN_CHOICES};
pub use flow::{
    CARD_LINE_CHARS, CARD_TITLE_CHARS, Flow, FlowCard, LAYER_SUBTITLE_CHARS, LAYER_TITLE_CHARS,
    Layer, MAX_CARDS, MAX_LAYERS, MIN_LAYERS,
};
pub use line::Line;
pub(crate) use line::is_plain;

use anyhow::bail;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::ToolCall;

/// The tool whose calls are blocks.
pub const SHOW_TOOL: &str = "show";

/// One block. It is deserialized only through `Show`, so one read from a call or from
/// history is within its caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", try_from = "Show")]
pub enum Block {
    Flow(Flow),
    Checks(Checks),
    Choices(Choices),
}

/// The arguments of `show`: exactly one kind. Flat, since providers want one plain
/// object at the top of a schema; it reads the same JSON a `Block` writes.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Show {
    pub flow: Option<Flow>,
    pub checks: Option<Checks>,
    pub choices: Option<Choices>,
}

impl TryFrom<Show> for Block {
    type Error = anyhow::Error;

    fn try_from(show: Show) -> anyhow::Result<Self> {
        let block = match (show.flow, show.checks, show.choices) {
            (Some(flow), None, None) => Self::Flow(flow),
            (None, Some(checks), None) => Self::Checks(checks),
            (None, None, Some(choices)) => Self::Choices(choices),
            _ => bail!("give exactly one of flow, checks and choices"),
        };
        match &block {
            Self::Flow(flow) => flow.check()?,
            Self::Checks(checks) => checks.check()?,
            Self::Choices(choices) => choices.check()?,
        }
        Ok(block)
    }
}

impl Block {
    /// The first line of `text`, by which a stored result is known to be a shown block.
    fn heading(&self) -> &'static str {
        match self {
            Self::Flow(_) => "Flow:",
            Self::Checks(_) => "Checks:",
            Self::Choices(_) => "Choices:",
        }
    }

    /// The block as plain text: what the model gets back from `show`, and what a client
    /// without a renderer for the kind shows.
    pub fn text(&self) -> String {
        let mut text = format!("{}\n", self.heading());
        match self {
            Self::Flow(flow) => flow.write(&mut text),
            Self::Checks(checks) => checks.write(&mut text),
            Self::Choices(choices) => choices.write(&mut text),
        }
        text.truncate(text.trim_end().len());
        text
    }

    /// The block a `show` call asks for, before its result is known: what a client keeps
    /// from `ToolStarted`, or from a call of an assistant message in history, when it
    /// does not keep the call itself. `None` for another tool and for a block out of caps.
    pub fn asked(call: &ToolCall) -> Option<Self> {
        if call.name != SHOW_TOOL {
            return None;
        }
        Self::deserialize(&call.arguments).ok()
    }

    /// Whether `result`, the content of the call's result or tool message, says the block
    /// was shown. A refused call (switched off, denied by a plugin, interrupted) has an
    /// error there instead, and stays a plain step.
    pub fn shown_by(&self, result: &str) -> bool {
        // Only the heading is compared, so the text below it may change between versions.
        let rest = result.strip_prefix(self.heading());
        rest.is_some_and(|rest| rest.is_empty() || rest.starts_with('\n'))
    }

    /// The block a `show` call put in the chat, given its result: `asked` and `shown_by`
    /// in one, for a client that has both at hand.
    pub fn shown(call: &ToolCall, result: &str) -> Option<Self> {
        Self::asked(call).filter(|block| block.shown_by(result))
    }
}

#[cfg(test)]
mod tests;
