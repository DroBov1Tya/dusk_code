//! A value that must not reach a log.

use serde::{Deserialize, Serialize};
use std::fmt;

/// A value that must not reach a log: it prints as hidden.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(pub String);

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(hidden)")
    }
}
