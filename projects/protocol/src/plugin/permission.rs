//! What a plugin asks to be allowed. Everything it does not ask for is refused, and the
//! user sees the list before the plugin is switched on.
//!
//! A plugin never reaches the network itself: its requests go through the core, which
//! lets them out only to the origins listed here and puts the plugin's keys into them on
//! the way. So a key can only ever travel to the origin it was given for.

use serde::{Deserialize, Serialize};

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Permissions {
    /// Origins its requests may go to through the core, such as
    /// `https://api.telegram.org`: scheme, host and port, no path.
    pub network: Vec<String>,
    /// Programs it may start besides its own, by name, such as `xcrun`.
    pub exec: Vec<String>,
    /// Folders outside its own it may read, such as `~/Library/Developer`.
    pub read: Vec<String>,
    /// Folders whose Unix sockets it may connect to, such as `~/.vphone`: the way a
    /// local program such as a virtual machine takes commands.
    pub sockets: Vec<String>,
    /// Keys of its own the user gives it, and where each may be sent.
    pub keys: Vec<KeyUse>,
    /// Only for a plugin that provides a contract: the folder of the project a call is
    /// about, for that call only.
    pub project: ProjectAccess,
    /// No longer granted: a manifest that asks for it is refused. Kept so that such a
    /// manifest is refused with a reason rather than as an unknown field.
    pub keychain: bool,
}

/// How much of the project a contract call is about the plugin may reach.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectAccess {
    #[default]
    None,
    Read,
}

/// One of the plugin's own keys. The user's provider keys are never among them.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeyUse {
    pub name: String,
    /// The origins the core puts the key into requests for; each is also in `network`.
    pub origins: Vec<String>,
    /// The plugin reads the key itself, for a protocol the core cannot carry. It could
    /// then send the key to any origin it may reach, so the user is told so plainly.
    pub handed: bool,
}

impl Permissions {
    /// One line per permission, as the user is asked.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for origin in &self.network {
            lines.push(format!("connect to {origin}"));
        }
        for program in &self.exec {
            lines.push(match program.as_str() {
                "xcrun" => "run xcrun, and through it the developer tools of Xcode".into(),
                _ => format!("run {program}"),
            });
        }
        for folder in &self.read {
            lines.push(format!("read {folder}"));
        }
        for folder in &self.sockets {
            lines.push(format!("talk to programs through the sockets in {folder}"));
        }
        for key in &self.keys {
            let to = key.origins.join(", ");
            lines.push(match key.handed {
                false => format!("use your {} with {to}; it never sees the key", key.name),
                true => format!(
                    "read your {} itself, to use with {to}; it could send it anywhere it may connect",
                    key.name
                ),
            });
        }
        if self.project == ProjectAccess::Read {
            lines.push("read the folder of the project it is asked about, while it answers".into());
        }
        if !self.network.is_empty()
            && (self.project == ProjectAccess::Read || !self.read.is_empty())
        {
            // The list limits where it connects, not what it sends there.
            lines.push(
                "send what it reads to those addresses, under any account it can use there".into(),
            );
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_project_has_its_line_and_the_network_says_what_it_carries() {
        assert!(Permissions::default().lines().is_empty());
        let read: Permissions = serde_json::from_str(r#"{"project":"read"}"#).unwrap();
        assert_eq!(read.lines().len(), 1);
        let sends: Permissions =
            serde_json::from_str(r#"{"project":"read","network":["https://api.github.com"]}"#)
                .unwrap();
        assert!(
            sends.lines()[2].starts_with("send what it reads"),
            "{:?}",
            sends.lines()
        );
        assert!(serde_json::from_str::<Permissions>(r#"{"project":"write"}"#).is_err());
    }
}
