//! What a plugin's manifest says it implements: one optional field per contract the
//! core knows. The values are checked when the manifest loads; this is their shape.

use std::collections::BTreeMap;

use serde::de::IgnoredAny;
use serde::{Deserialize, Serialize};

/// No `deny_unknown_fields`: `unknown` catches a contract this core does not know, so
/// the manifest's error can name it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Provides {
    /// The contract program, one for all the plugin's contracts; `./` is the plugin's
    /// folder.
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub forge: Option<ForgeProvides>,
    #[serde(flatten, skip_serializing)]
    pub unknown: BTreeMap<String, IgnoredAny>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ForgeProvides {
    /// Contract versions the program speaks.
    pub versions: Vec<u32>,
    /// Git remote hosts it serves.
    pub hosts: Vec<String>,
}

impl Provides {
    /// One line per contract, as the user is asked.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(forge) = &self.forge {
            lines.push(format!(
                "serves pull requests and CI for {}",
                forge.hosts.join(", ")
            ));
        }
        lines
    }

    /// Whether `self` serves every contract and host `wanted` does; an update that
    /// fails this asks for more than the installed plugin was allowed.
    pub fn covers(&self, wanted: &Provides) -> bool {
        match (&wanted.forge, &self.forge) {
            (None, _) => true,
            (Some(_), None) => false,
            (Some(want), Some(have)) => want.hosts.iter().all(|host| have.hosts.contains(host)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(yaml: &str) -> Provides {
        serde_json::from_str(yaml).unwrap()
    }

    #[test]
    fn unknown_contract_is_named_and_unknown_field_refused() {
        let provides = read(r#"{"command":["./p"],"speech":{"x":1}}"#);
        assert_eq!(provides.unknown.keys().collect::<Vec<_>>(), ["speech"]);
        assert!(
            serde_json::from_str::<Provides>(r#"{"forge":{"versions":[1],"hosts":[],"x":1}}"#)
                .is_err()
        );
    }

    #[test]
    fn lines_and_covers() {
        let forge = |hosts: &[&str]| Provides {
            forge: Some(ForgeProvides {
                versions: vec![1],
                hosts: hosts.iter().map(|h| h.to_string()).collect(),
            }),
            ..Default::default()
        };
        let both = forge(&["a.com", "b.com"]);
        assert_eq!(
            both.lines(),
            ["serves pull requests and CI for a.com, b.com"]
        );
        assert!(both.covers(&forge(&["b.com"])));
        assert!(!forge(&["a.com"]).covers(&both));
        assert!(!Provides::default().covers(&both));
        assert!(Provides::default().covers(&Provides::default()));
    }
}
