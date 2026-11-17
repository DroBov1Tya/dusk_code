use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::Line;

pub const MIN_CHOICES: usize = 2;
pub const MAX_CHOICES: usize = 4;
pub const CHOICE_CHARS: usize = 24;

// Plain comments on the types here: a doc comment would go into the schema of `show`
// and so into every request.

// Reply buttons under the answer. A click sends the label as the user's next message;
// nothing waits for it, unlike a `Question`. Built and read only through `new`, so one
// that exists is within its caps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "ChoicesFields")]
pub struct Choices {
    labels: Vec<Line<CHOICE_CHARS>>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ChoicesFields {
    /// The one you recommend first.
    #[schemars(length(min = MIN_CHOICES, max = MAX_CHOICES))]
    labels: Vec<Line<CHOICE_CHARS>>,
}

impl Choices {
    pub fn new(labels: Vec<Line<CHOICE_CHARS>>) -> anyhow::Result<Self> {
        ensure!(
            (MIN_CHOICES..=MAX_CHOICES).contains(&labels.len()),
            "give {MIN_CHOICES} to {MAX_CHOICES} choices"
        );
        for (index, label) in labels.iter().enumerate() {
            ensure!(
                !labels[..index].contains(label),
                "choice {} repeats an earlier one",
                index + 1
            );
        }
        Ok(Self { labels })
    }

    pub fn labels(&self) -> &[Line<CHOICE_CHARS>] {
        &self.labels
    }

    pub(super) fn write(&self, text: &mut String) {
        for (index, label) in self.labels.iter().enumerate() {
            text.push_str(&format!("{}. {label}\n", index + 1));
        }
    }
}

impl TryFrom<ChoicesFields> for Choices {
    type Error = anyhow::Error;

    fn try_from(fields: ChoicesFields) -> anyhow::Result<Self> {
        Self::new(fields.labels)
    }
}
