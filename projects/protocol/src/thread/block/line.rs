use std::borrow::Cow;

use anyhow::ensure;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize};

/// How much of a refused text its error quotes.
const QUOTED_CHARS: usize = 24;

/// One line of plain text of at most `MAX` characters: trimmed, not empty, and free of
/// control and text-direction characters. A client draws it as it is, never as markdown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Line<const MAX: usize>(String);

impl<const MAX: usize> Line<MAX> {
    pub fn new(text: &str) -> anyhow::Result<Self> {
        let text = text.trim();
        ensure!(!text.is_empty(), "a text is empty");
        let quoted: String = text.chars().take(QUOTED_CHARS).collect();
        ensure!(
            text.chars().count() <= MAX,
            "the text starting {quoted:?} is over {MAX} characters"
        );
        ensure!(
            crate::text::is_plain_line(text),
            "the text starting {quoted:?} must be one line without control characters"
        );
        Ok(Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<const MAX: usize> std::fmt::Display for Line<MAX> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de, const MAX: usize> Deserialize<'de> for Line<MAX> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::new(&text).map_err(serde::de::Error::custom)
    }
}

// The model reads the cap in the tool's schema, and a call over it is refused with the
// path of the field before the tool runs.
impl<const MAX: usize> JsonSchema for Line<MAX> {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        format!("Line{MAX}").into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "minLength": 1, "maxLength": MAX })
    }
}
