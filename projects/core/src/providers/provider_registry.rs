//! Named providers built from the config.

use std::collections::BTreeMap;

use anyhow::Result;

use super::{
    openai_compatible::OpenAiCompatible,
    provider::{Provider, ProviderAdapter},
};
use crate::config::{Config, ProviderKind};

/// Providers by config name, in name order.
pub struct ProviderRegistry {
    providers: BTreeMap<String, Provider>,
}

impl ProviderRegistry {
    /// Builds adapters without making any network requests.
    pub fn from_config(config: &Config) -> Result<Self> {
        let mut providers = BTreeMap::new();
        for (name, settings) in config.providers() {
            let adapter = match settings.kind {
                ProviderKind::OpenAiCompatible => {
                    ProviderAdapter::OpenAiCompatible(OpenAiCompatible::new(settings)?)
                }
            };
            providers.insert(name.clone(), Provider::new(adapter));
        }
        Ok(Self { providers })
    }

    /// Provider names in order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.providers.keys().map(String::as_str)
    }

    pub fn provider(&self, name: &str) -> Option<&Provider> {
        self.providers.get(name)
    }
}
