//! Extension metadata and registry storage, independent of plugin loading.

use std::collections::BTreeMap;

use crate::api::ApiVersion;

#[derive(Debug, Clone)]
pub struct ExtensionDescriptor {
    pub id: String,
    pub required_core_api: ApiVersion,
}

pub type ExtensionRegistry = BTreeMap<String, ExtensionDescriptor>;

// Registration validation and extension lifecycle hooks belong in this module.
