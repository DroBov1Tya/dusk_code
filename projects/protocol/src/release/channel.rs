//! Which releases are followed.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Stable,
    Dev,
}

impl Channel {
    pub fn name(self) -> &'static str {
        match self {
            Self::Stable => "stable",
            Self::Dev => "dev",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Stable => Self::Dev,
            Self::Dev => Self::Stable,
        }
    }
}
