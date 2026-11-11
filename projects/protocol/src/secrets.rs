//! What a client may know of the secrets: which exist, never their values.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsInfo {
    /// The system keychain holds the secrets; without one, provider keys stay in the
    /// private config file and the other secrets cannot be kept.
    pub keychain: bool,
    /// Why the keychain could not be read, such as a lock the user has not opened.
    pub problem: Option<String>,
    pub entries: Vec<SecretInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretInfo {
    pub name: String,
    pub label: String,
    pub set: bool,
    /// The user sets and clears it here; a provider's key goes with the provider.
    pub editable: bool,
}

/// The secret of the Hugging Face token, for downloading models.
pub const HUGGINGFACE: &str = "huggingface";
