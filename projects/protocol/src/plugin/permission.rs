//! What a plugin asks to be allowed. Everything it does not ask for is refused, and the
//! user sees the list before the plugin is switched on.
//!
//! A plugin never reaches the network itself: its requests go through the core, which
//! lets them out only to the origins listed here and puts the plugin's keys into them on
//! the way. So a key can only ever travel to the origin it was given for.

use serde::{Deserialize, Serialize};

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
    /// Keys of its own the user gives it, and where each may be sent.
    pub keys: Vec<KeyUse>,
}

/// One of the plugin's own keys. The user's provider keys are never among them.
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
            lines.push(format!("run {program}"));
        }
        for folder in &self.read {
            lines.push(format!("read {folder}"));
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
        lines
    }
}
