//! Connecting and forgetting providers.

use crate::runtime::Runtime;
use crate::{
    config::{ApiKey, ProviderConfig, ProviderKind},
    providers::Provider,
};
use anyhow::{Result, ensure};
use std::sync::Arc;

/// A provider's name is this long at most; it shows in `provider/model`.
const MAX_PROVIDER_NAME: usize = 40;

/// A key or token as it is kept: trimmed, since a pasted one often carries a newline or a
/// space, and refused when it is not plain text of a sane size.
pub(super) fn checked_key(key: String) -> Result<String> {
    let key = key.trim().to_owned();
    ensure!(
        key.len() <= 4096 && !key.chars().any(char::is_control),
        "a key is text of up to 4096 characters, without control characters"
    );
    Ok(key)
}

impl Runtime {
    /// Adds a provider to the private YAML config under `name`, or the next free
    /// `provider-N`, and returns the name. A model is chosen separately through
    /// `/model`; saving makes no network request.
    pub fn connect_provider(
        &self,
        kind: ProviderKind,
        name: Option<String>,
        base_url: String,
        api_key: Option<String>,
    ) -> Result<String> {
        if let Some(name) = &name {
            ensure!(
                !name.trim().is_empty()
                    && name.len() <= MAX_PROVIDER_NAME
                    && !name.contains('/')
                    && !name.chars().any(char::is_control),
                "a name is up to {MAX_PROVIDER_NAME} characters, without / or control characters"
            );
        }
        let provider = ProviderConfig {
            kind,
            base_url,
            // A pasted key often carries a newline or a space, which the provider rejects.
            api_key: ApiKey(checked_key(api_key.unwrap_or_default())?),
            inline_tags: false,
            allow_http: false,
            prices: Default::default(),
            balance: None,
            requests_per_minute: None,
        };
        crate::providers::validate_provider(&provider)?;
        let adapter = Arc::new(Provider::new(&provider)?);
        let mut chosen = Err(anyhow::anyhow!("a provider with this name exists"));
        self.launcher.config.update(|config| {
            let free = |name: &String| !config.providers().contains_key(name);
            let name = match &name {
                Some(name) if !free(name) => return,
                Some(name) => name.clone(),
                None => (1..)
                    .map(|index| format!("provider-{index}"))
                    .find(free)
                    .expect("a free provider name"),
            };
            config.set_provider(name.clone(), provider);
            chosen = Ok(name);
        })?;
        let name = chosen?;
        self.launcher.model.replace_provider(&name, adapter);
        Ok(name)
    }

    /// Forgets a provider and its key. The provider of the chosen model stays: the chat
    /// would lose its model under the user.
    pub fn remove_provider(&self, name: &str) -> Result<()> {
        let config = self.launcher.config.current();
        ensure!(
            config.providers().contains_key(name),
            "no provider named {name}"
        );
        let in_use = config
            .model()
            .is_some_and(|model| model.starts_with(&format!("{name}/")));
        ensure!(
            !in_use,
            "{name} holds the chosen model; choose a model of another provider first"
        );
        self.launcher.config.update(|config| {
            config.remove_provider(name);
        })?;
        Ok(())
    }

    /// Forgets every provider and its key, and with them the chosen model.
    pub fn remove_all_providers(&self) -> Result<()> {
        self.launcher
            .config
            .update(|config| config.remove_all_providers())?;
        self.launcher.model.clear();
        Ok(())
    }
}
