//! `/model [provider/model]`: without an argument, every configured provider checked
//! live, with its models; with one, the open chat and the chats started from now on
//! switch to that model. Other chats keep theirs.

use anyhow::{Context, Result};

use crate::providers::ProviderRegistry;
use crate::runtime::{
    Runtime,
    model_choice::{Choice, ProviderStatus, check_provider, check_providers},
};

pub async fn check(runtime: &Runtime) -> Result<Vec<ProviderStatus>> {
    let statuses = check_providers(&runtime.config().current()).await?;
    runtime.learn_model(&statuses);
    Ok(statuses)
}

/// The providers that are switched on, found without asking any of them: no models yet.
pub fn unasked(runtime: &Runtime) -> Result<Vec<ProviderStatus>> {
    let config = runtime.config().current();
    let registry = ProviderRegistry::from_config(&config)?;
    let all = config.all_providers();
    let mut names: Vec<&str> = registry.names().collect();
    names.sort();
    Ok(names
        .into_iter()
        .map(|name| ProviderStatus {
            name: name.to_owned(),
            models: Ok(Vec::new()),
            balance: None,
            limits: None,
            enabled: true,
            failure: None,
            hidden: all
                .get(name)
                .map(|settings| settings.hidden_models.iter().cloned().collect())
                .unwrap_or_default(),
        })
        .collect())
}

/// Asks one provider for its models, so a slow one does not hold back the others.
pub async fn check_one(runtime: &Runtime, name: &str) -> Result<ProviderStatus> {
    let config = runtime.config().current();
    let registry = ProviderRegistry::from_config(&config)?;
    let provider = registry
        .provider(name)
        .with_context(|| format!("no provider named {name}"))?;
    let status = check_provider(
        name.to_owned(),
        provider,
        config.all_providers().get(name).cloned(),
    )
    .await;
    runtime.learn_model(std::slice::from_ref(&status));
    Ok(status)
}

pub async fn switch(runtime: &Runtime, thread_id: Option<&str>, name: &str) -> Result<Choice> {
    let choice = Choice::pick(&runtime.config().current(), Some(name)).await?;
    runtime.set_model(thread_id, choice.clone()).await?;
    Ok(choice)
}
