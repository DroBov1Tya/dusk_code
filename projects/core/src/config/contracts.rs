//! The `contracts:` section: the plugin the user chose for a host, by contract. The
//! switch `plugin_contracts` that turns contract programs on lives beside it at the top
//! of the file.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// What the user chose, by contract and then host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contracts {
    /// The forge plugin for a host, such as `github.com`: its folder name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub forge: BTreeMap<String, String>,
}

impl Contracts {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}
