//! The provider and model threads talk to. `/model` switches it for every thread at once;
//! a model call that has started finishes with the model it started with. The choice is
//! kept in the config as `model: provider/model`, so it outlives a restart.

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use parking_lot::RwLock;

use anyhow::{Context, Result, anyhow};
use tokio::task::JoinSet;

use crate::{
    config::Config,
    providers::{Model, ModelProvider, ProviderRegistry},
};

pub use dusk_protocol::providers::ProviderStatus;

/// A provider that does not list its models within this time counts as down.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// How long a model whose vision could not be checked waits to be asked again.
const VISION_RETRY: Duration = Duration::from_secs(10 * 60);

pub struct ModelChoice {
    current: RwLock<Selection>,
    /// Whether each model, by `provider/model`, saw the test picture.
    vision: RwLock<HashMap<String, bool>>,
    /// When asking a model failed, so it is not asked again at every step.
    vision_failed: RwLock<HashMap<String, Instant>>,
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

    /// Reloads metadata for a saved selection without replacing its adapter.
    pub async fn refresh(&self) -> Result<Model> {
        tokio::time::timeout(CHECK_TIMEOUT, self.provider.list_models())
            .await
            .context("model metadata refresh timed out")??
            .into_iter()
            .find(|model| model.id == self.model.id)
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
                max_output: None,
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
                match Self::first_of(&registry, provider_name).await {
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
        let model = Self::models(&*provider, provider_name)
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

    /// A provider's catalogue, given up on when it does not answer soon: a dead server must
    /// not hold `/model` or the start's discovery until the transport gives up.
    async fn models(provider: &dyn ModelProvider, provider_name: &str) -> Result<Vec<Model>> {
        tokio::time::timeout(CHECK_TIMEOUT, provider.list_models())
            .await
            .with_context(|| format!("provider {provider_name} did not answer in time"))?
    }

    async fn first_of(registry: &ProviderRegistry, provider_name: &str) -> Result<Choice> {
        let provider = registry
            .provider(provider_name)
            .context("provider is missing")?;
        let model = Self::models(&*provider, provider_name)
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
}

impl ModelChoice {
    pub fn new(choice: Choice) -> Self {
        Self {
            current: RwLock::new(Selection {
                choice: Some(choice),
                revision: Arc::new(()),
            }),
            vision: RwLock::default(),
            vision_failed: RwLock::default(),
        }
    }

    pub fn offline() -> Self {
        Self {
            current: RwLock::new(Selection {
                choice: None,
                revision: Arc::new(()),
            }),
            vision: RwLock::default(),
            vision_failed: RwLock::default(),
        }
    }

    /// `None` until the model was asked.
    pub fn vision(&self, model: &str) -> Option<bool> {
        self.vision.read().get(model).copied()
    }

    pub fn set_vision(&self, model: String, sees: bool) {
        self.vision_failed.write().remove(&model);
        self.vision.write().insert(model, sees);
    }

    /// Whether the model may be asked again after a failed ask.
    pub fn may_ask_vision(&self, model: &str) -> bool {
        self.vision_failed
            .read()
            .get(model)
            .is_none_or(|failed| failed.elapsed() >= VISION_RETRY)
    }

    pub fn vision_failed(&self, model: String) {
        self.vision_failed.write().insert(model, Instant::now());
    }

    pub fn current(&self) -> Option<Choice> {
        self.current.read().choice.clone()
    }

    pub fn set(&self, choice: Choice) {
        let mut current = self.current.write();
        current.choice = Some(choice);
        current.revision = Arc::new(());
    }

    pub fn revision(&self) -> Arc<()> {
        Arc::clone(&self.current.read().revision)
    }

    pub fn set_if_empty_at_revision(&self, choice: Choice, revision: &Arc<()>) -> bool {
        let mut current = self.current.write();
        if current.choice.is_some() || !Arc::ptr_eq(&current.revision, revision) {
            return false;
        }
        current.choice = Some(choice);
        current.revision = Arc::new(());
        true
    }

    pub fn replace_provider(&self, name: &str, provider: Arc<dyn ModelProvider>) {
        let mut current = self.current.write();
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
        let mut current = self.current.write();
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
