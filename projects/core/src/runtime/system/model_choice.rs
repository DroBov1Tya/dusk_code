//! The provider and model threads talk to. `/model` switches it for every thread at once;
//! a model call that has started finishes with the model it started with. The choice is
//! kept in the config as `model: provider/model`, so it outlives a restart.

use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use tokio::task::JoinSet;

use crate::{
    config::Config,
    providers::{Model, ModelProvider, ProviderRegistry},
    util::sync::{read, write},
};

/// A provider that does not list its models within this time counts as down.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// A configured provider as a check found it: its models, or why it did not answer.
pub struct ProviderStatus {
    pub name: String,
    pub models: Result<Vec<Model>, String>,
}

pub struct ModelChoice {
    current: RwLock<Selection>,
}

struct Selection {
    choice: Option<Choice>,
    revision: Arc<()>,
}

#[derive(Clone)]
pub struct Choice {
    /// The provider's name in the config.
    pub provider_name: String,
    pub provider: Arc<dyn ModelProvider>,
    pub model: Model,
}

impl Choice {
    pub fn input_limit(&self, config: &Config) -> u64 {
        config.input_limit(
            &self.provider_name,
            &self.model.id,
            self.model.context_window,
        )
    }

    /// How the config and clients write it: `provider/model`.
    pub fn name(&self) -> String {
        format!("{}/{}", self.provider_name, self.model.id)
    }
}

impl ModelChoice {
    pub fn new(choice: Choice) -> Self {
        Self {
            current: RwLock::new(Selection {
                choice: Some(choice),
                revision: Arc::new(()),
            }),
        }
    }

    pub fn offline() -> Self {
        Self {
            current: RwLock::new(Selection {
                choice: None,
                revision: Arc::new(()),
            }),
        }
    }

    pub fn current(&self) -> Option<Choice> {
        read(&self.current).choice.clone()
    }

    pub fn set(&self, choice: Choice) {
        let mut current = write(&self.current);
        current.choice = Some(choice);
        current.revision = Arc::new(());
    }

    pub fn set_if_empty(&self, choice: Choice) -> bool {
        let mut current = write(&self.current);
        if current.choice.is_some() {
            return false;
        }
        current.choice = Some(choice);
        current.revision = Arc::new(());
        true
    }

    pub fn revision(&self) -> Arc<()> {
        Arc::clone(&read(&self.current).revision)
    }

    pub fn set_if_empty_at_revision(&self, choice: Choice, revision: &Arc<()>) -> bool {
        let mut current = write(&self.current);
        if current.choice.is_some() || !Arc::ptr_eq(&current.revision, revision) {
            return false;
        }
        current.choice = Some(choice);
        current.revision = Arc::new(());
        true
    }

    pub fn replace_provider(&self, name: &str, provider: Arc<dyn ModelProvider>) {
        let mut current = write(&self.current);
        // Even an offline selection must reject discovery started before reconnect.
        current.revision = Arc::new(());
        if let Some(choice) = current
            .choice
            .as_mut()
            .filter(|choice| choice.provider_name == name)
        {
            choice.provider = provider;
        }
    }

    /// A catalogue response applies only to the selection and adapter it checked.
    pub fn refresh_model(&self, expected: &Choice, model: Model) -> Option<Choice> {
        let mut current = write(&self.current);
        let choice = current.choice.as_mut()?;
        if choice.provider_name != expected.provider_name
            || choice.model.id != expected.model.id
            || !Arc::ptr_eq(&choice.provider, &expected.provider)
        {
            return None;
        }
        choice.model = model;
        Some(choice.clone())
    }
}

/// Reloads metadata for a saved selection without replacing its adapter.
pub async fn refresh(choice: &Choice) -> Result<Model> {
    tokio::time::timeout(CHECK_TIMEOUT, choice.provider.list_models())
        .await
        .context("model metadata refresh timed out")??
        .into_iter()
        .find(|model| model.id == choice.model.id)
        .context("saved model is missing from the provider catalogue")
}

/// Constructs a saved model without asking the provider for its catalogue. This keeps
/// local history available while the provider is down.
pub fn configured(config: &Config) -> Result<Option<Choice>> {
    let Some(name) = config.model() else {
        return Ok(None);
    };
    let Some((provider_name, id)) = name.split_once('/') else {
        return Ok(None);
    };
    if provider_name.is_empty() || id.trim().is_empty() {
        return Ok(None);
    }
    let registry = ProviderRegistry::from_config(config)?;
    let Some(provider) = registry.provider(provider_name) else {
        return Ok(None);
    };
    Ok(Some(Choice {
        provider_name: provider_name.to_owned(),
        provider,
        model: Model {
            id: id.to_owned(),
            context_window: None,
        },
    }))
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
