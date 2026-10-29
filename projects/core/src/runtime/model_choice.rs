//! The provider and model threads talk to. `/model` switches it for every thread at once;
//! a model call that has started finishes with the model it started with. The choice is
//! kept in the config as `model: provider/model`, so it outlives a restart.

use std::{
    sync::{Arc, PoisonError, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use tokio::task::JoinSet;

use crate::{
    config::Config,
    providers::{Model, ModelProvider, ProviderRegistry},
};

/// A provider that does not list its models within this time counts as down.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// A configured provider as a check found it: its models, or why it did not answer.
pub struct ProviderStatus {
    pub name: String,
    pub models: Result<Vec<Model>, String>,
}

pub struct ModelChoice {
    current: RwLock<Choice>,
}

#[derive(Clone)]
pub struct Choice {
    /// The provider's name in the config.
    pub provider_name: String,
    pub provider: Arc<dyn ModelProvider>,
    pub model: Model,
}

impl Choice {
    /// How the config and clients write it: `provider/model`.
    pub fn name(&self) -> String {
        format!("{}/{}", self.provider_name, self.model.id)
    }
}

impl ModelChoice {
    pub fn new(choice: Choice) -> Self {
        Self {
            current: RwLock::new(choice),
        }
    }

    pub fn current(&self) -> Choice {
        // A choice is replaced whole, so a panic elsewhere cannot leave it half-written.
        self.current
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub fn set(&self, choice: Choice) {
        *self.current.write().unwrap_or_else(PoisonError::into_inner) = choice;
    }
}

/// `name` as `provider/model`, or when `None` the first model of the first provider that
/// answers.
pub async fn pick(config: &Config, name: Option<&str>) -> Result<Choice> {
    let registry = ProviderRegistry::from_config(config)?;
    let Some(name) = name else {
        let mut last_error = None;
        for provider_name in registry.names() {
            match first_model(&registry, provider_name).await {
                Ok(choice) => return Ok(choice),
                Err(error) => last_error = Some(error),
            }
        }
        return Err(last_error.unwrap_or_else(|| anyhow!("no provider configured")));
    };
    let (provider_name, id) = name
        .split_once('/')
        .with_context(|| format!("{name} is not provider/model"))?;
    let provider = registry
        .provider(provider_name)
        .with_context(|| format!("no provider {provider_name} in the config"))?;
    let model = provider
        .list_models()
        .await?
        .into_iter()
        .find(|model| model.id == id)
        .with_context(|| format!("provider {provider_name} offers no model {id}"))?;
    Ok(Choice {
        provider_name: provider_name.to_owned(),
        provider,
        model,
    })
}

async fn first_model(registry: &ProviderRegistry, provider_name: &str) -> Result<Choice> {
    let provider = registry
        .provider(provider_name)
        .context("provider is missing")?;
    let model = provider
        .list_models()
        .await?
        .into_iter()
        .next()
        .with_context(|| format!("provider {provider_name} offers no models"))?;
    Ok(Choice {
        provider_name: provider_name.to_owned(),
        provider,
        model,
    })
}

/// Asks every configured provider for its models at once.
pub async fn check_providers(config: &Config) -> Result<Vec<ProviderStatus>> {
    let registry = ProviderRegistry::from_config(config)?;
    let mut checks = JoinSet::new();
    for name in registry.names() {
        let name = name.to_owned();
        let Some(provider) = registry.provider(&name) else {
            continue;
        };
        checks.spawn(async move {
            let listed = tokio::time::timeout(CHECK_TIMEOUT, provider.list_models()).await;
            let models = match listed {
                Ok(Ok(models)) => Ok(models),
                Ok(Err(error)) => Err(format!("{error:#}")),
                Err(_) => Err("no answer in time".into()),
            };
            ProviderStatus { name, models }
        });
    }
    let mut statuses = checks.join_all().await;
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(statuses)
}
