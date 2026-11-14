//! The plugin store: what it offers, as the core read it from the signed list, and what
//! a client asks of it. A client only shows the list; checking the signature, the
//! download and the files is the core's.

use serde::{Deserialize, Serialize};

use super::{Client, Os};

/// One plugin the store offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub name: String,
    pub version: String,
    pub description: String,
    /// All when empty.
    pub platforms: Vec<Os>,
    /// All when empty.
    pub clients: Vec<Client>,
    /// What it may do, one line each, as the user is asked before it is installed.
    pub permissions: Vec<String>,
    /// An `https` page about it.
    pub homepage: Option<String>,
    /// The version here, when it is installed from the store.
    pub installed: Option<String>,
    /// Why it cannot be installed on this machine, when it cannot.
    pub unfit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoreRequest {
    /// The store's plugins; answered with `Response::Store`.
    List,
    /// Installs or updates `name` at `version` and switches it on. `agreed` is the
    /// listing's `permissions` the user was shown; a listing that changed since is
    /// refused.
    Install {
        name: String,
        version: String,
        agreed: Vec<String>,
    },
    /// Removes a plugin installed from the store.
    Remove { name: String },
}
