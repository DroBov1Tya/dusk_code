//! The provider and model threads talk to. The runtime's selection is the one last
//! chosen, kept in the config as `model: provider/model`: a new chat starts with it. Each
//! chat has a selection of its own (`ModelChoice::own`), which its agents start from, so
//! `/model` in one chat never switches another. A model call that has started finishes
//! with the model it started with.

use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, Instant},
};

use parking_lot::{Mutex, RwLock};

use dusk_protocol::model::Effort;

use anyhow::{Context, Result, anyhow, ensure};
use tokio::task::JoinSet;

use crate::{
    config::Config,
    providers::{Model, ModelProvider, ProviderRegistry},
};

pub use dusk_protocol::model::ProviderStatus;

/// A provider that does not list its models within this time counts as down.
const CHECK_TIMEOUT: Duration = Duration::from_secs(8);

/// How long a model whose vision could not be checked waits to be asked again.
const VISION_RETRY: Duration = Duration::from_secs(10 * 60);

pub struct ModelChoice {
    current: RwLock<Selection>,
    shared: Arc<Shared>,
}

/// What the runtime's selection and every thread's own one share.
#[derive(Default)]
struct Shared {
    /// Whether each model, by `provider/model`, saw the test picture.
    vision: RwLock<HashMap<String, bool>>,
    /// When asking a model failed, so it is not asked again at every step.
    vision_failed: RwLock<HashMap<String, Instant>>,
    /// The threads' own selections, which a provider reconnected or removed reaches too.
    own: Mutex<Vec<Weak<ModelChoice>>>,
}

struct Selection {
    choice: Option<Choice>,
    /// A chat's own effort; `None` follows the settings.
    effort: Option<Option<Effort>>,
    /// The effort before the last change, for a model that refuses `off`.
    previous_effort: Option<Option<Effort>>,
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
        match config.model() {
            Some(name) => Self::named(config, name),
            None => Ok(None),
        }
    }

    /// `provider/model` of a configured provider that is switched on, built the same way
    /// without asking the provider; `None` when no such provider is left.
    pub fn named(config: &Config, name: &str) -> Result<Option<Choice>> {
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
        // Not every server lists its models: a name it does not list is still tried,
        // with its limits unknown, and the first request says if it is wrong.
        let listed = Self::models(&*provider, provider_name)
            .await
            .unwrap_or_default()
            .into_iter()
            .find(|model| model.id == id);
        ensure!(!id.trim().is_empty(), "the model name is empty");
        let model = listed.unwrap_or_else(|| Model {
            id: id.to_owned(),
            context_window: None,
            max_output: None,
        });
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
        Self::with(Some(choice), Arc::default())
    }

    pub fn offline() -> Self {
        Self::with(None, Arc::default())
    }

    fn with(choice: Option<Choice>, shared: Arc<Shared>) -> Self {
        Self {
            current: RwLock::new(Selection {
                choice,
                effort: None,
                previous_effort: None,
                revision: Arc::new(()),
            }),
            shared,
        }
    }

    /// A thread's own selection, starting at `choice`: setting it changes no other
    /// thread, while a provider reconnected or removed reaches it as it reaches this one.
    pub fn own(&self, choice: Option<Choice>) -> Arc<Self> {
        let own = Arc::new(Self::with(choice, Arc::clone(&self.shared)));
        let mut all = self.shared.own.lock();
        all.retain(|weak| weak.strong_count() > 0);
        all.push(Arc::downgrade(&own));
        own
    }

    /// The threads' own selections still in use.
    fn owned(&self) -> Vec<Arc<Self>> {
        self.shared
            .own
            .lock()
            .iter()
            .filter_map(Weak::upgrade)
            .collect()
    }

    /// `provider/model` with its sizes as some selection learned them, else `None`.
    pub fn known(&self, name: &str) -> Option<Choice> {
        std::iter::once(self.current())
            .chain(self.owned().iter().map(|own| own.current()))
            .flatten()
            .filter(|choice| choice.name() == name)
            .max_by_key(|choice| {
                u8::from(choice.model.context_window.is_some())
                    + u8::from(choice.model.max_output.is_some())
            })
    }

    /// `None` until the model was asked.
    pub fn vision(&self, model: &str) -> Option<bool> {
        self.shared.vision.read().get(model).copied()
    }

    pub fn set_vision(&self, model: String, sees: bool) {
        self.shared.vision_failed.write().remove(&model);
        self.shared.vision.write().insert(model, sees);
    }

    /// Whether the model may be asked again after a failed ask.
    pub fn may_ask_vision(&self, model: &str) -> bool {
        self.shared
            .vision_failed
            .read()
            .get(model)
            .is_none_or(|failed| failed.elapsed() >= VISION_RETRY)
    }

    pub fn vision_failed(&self, model: String) {
        self.shared
            .vision_failed
            .write()
            .insert(model, Instant::now());
    }

    pub fn current(&self) -> Option<Choice> {
        self.current.read().choice.clone()
    }

    /// The chat's own effort; `None` follows the settings, `Some(None)` is the provider's.
    pub fn effort(&self) -> Option<Option<Effort>> {
        self.current.read().effort
    }

    pub fn set_effort(&self, effort: Option<Option<Effort>>) {
        let mut current = self.current.write();
        if current.effort != effort {
            current.previous_effort = current.effort;
            current.effort = effort;
        }
    }

    /// A model refused `off`: a chat whose own effort is `off` goes back to the one before.
    /// The effort it has now, when it changed.
    pub fn restore_unsupported_effort(&self) -> Option<Option<Effort>> {
        let mut current = self.current.write();
        if current.effort != Some(Some(Effort::Off)) {
            return None;
        }
        let previous = current
            .previous_effort
            .filter(|effort| *effort != Some(Effort::Off));
        let restored = previous.unwrap_or(None);
        current.previous_effort = current.effort;
        current.effort = Some(restored);
        Some(restored)
    }

    /// The effort this selection thinks at under `config`.
    pub fn effort_in(&self, config: &Config) -> Option<Effort> {
        self.effort().unwrap_or_else(|| config.effort())
    }

    /// The largest request the current model takes under `config`'s policy.
    pub fn current_input_limit(&self, config: &Config) -> u64 {
        self.current()
            .map_or(crate::config::UNKNOWN_WINDOW, |choice| {
                choice.input_limit(config)
            })
    }

    /// Refuses a message that alone is past the input limit: it could never be answered.
    pub fn ensure_fits(&self, text: &str, config: &Config) -> Result<()> {
        let limit = self.current_input_limit(config);
        let tokens = crate::state::estimate_tokens(text);
        ensure!(
            tokens <= limit,
            "the message is about {tokens} tokens, past the input limit of {limit}"
        );
        Ok(())
    }

    pub fn set(&self, choice: Choice) {
        let mut current = self.current.write();
        current.choice = Some(choice);
        current.revision = Arc::new(());
    }

    /// No model is selected, and discovery started before this must not select one.
    /// Every thread loses its own as well: no provider is left to ask.
    pub fn clear(&self) {
        for own in self.owned() {
            own.current.write().choice = None;
        }
        let mut current = self.current.write();
        current.choice = None;
        current.revision = Arc::new(());
    }

    /// Threads that have no model yet take `choice`: they never answered on another.
    /// Whether any did.
    pub fn fill_empty(&self, choice: &Choice) -> bool {
        let mut filled = false;
        for own in self.owned() {
            let mut current = own.current.write();
            if current.choice.is_none() {
                current.choice = Some(choice.clone());
                filled = true;
            }
        }
        filled
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
        for own in self.owned() {
            own.replace_own_provider(name, &provider);
        }
        // Even an offline selection must reject discovery started before reconnect.
        self.current.write().revision = Arc::new(());
        self.replace_own_provider(name, &provider);
    }

    fn replace_own_provider(&self, name: &str, provider: &Arc<dyn ModelProvider>) {
        let mut current = self.current.write();
        if let Some(choice) = current
            .choice
            .as_mut()
            .filter(|choice| choice.provider_name == name)
        {
            choice.provider = Arc::clone(provider);
        }
    }

    /// A catalogue response applies only to the selection and adapter it checked; the
    /// threads on that model learn its sizes too.
    pub fn refresh_model(&self, expected: &Choice, model: Model) -> Option<Choice> {
        for own in self.owned() {
            own.refresh_own(expected, &model);
        }
        self.refresh_own(expected, &model)
    }

    fn refresh_own(&self, expected: &Choice, model: &Model) -> Option<Choice> {
        let mut current = self.current.write();
        let choice = current.choice.as_mut()?;
        if choice.provider_name != expected.provider_name
            || choice.model.id != expected.model.id
            || !Arc::ptr_eq(&choice.provider, &expected.provider)
        {
            return None;
        }
        choice.model = model.clone();
        Some(choice.clone())
    }

    /// Takes the chosen model's sizes from a catalogue of its provider when they are not
    /// known yet: the start asks once, and a provider that was down then must not leave
    /// the model at the unknown window for good. The threads' own selections learn as
    /// well. Whether any of them learned something.
    pub fn learn(&self, statuses: &[ProviderStatus]) -> bool {
        let mut learned = false;
        for own in self.owned() {
            learned |= own.learn_own(statuses).is_some();
        }
        self.learn_own(statuses).is_some() || learned
    }

    fn learn_own(&self, statuses: &[ProviderStatus]) -> Option<Choice> {
        let mut current = self.current.write();
        let choice = current.choice.as_mut()?;
        if choice.model.context_window.is_some() && choice.model.max_output.is_some() {
            return None;
        }
        let listed = statuses
            .iter()
            .filter(|status| status.name == choice.provider_name)
            .filter_map(|status| status.models.as_ref().ok())
            .flatten()
            .find(|model| model.id == choice.model.id)?;
        let context_window = choice.model.context_window.or(listed.context_window);
        let max_output = choice.model.max_output.or(listed.max_output);
        if context_window == choice.model.context_window && max_output == choice.model.max_output {
            return None;
        }
        choice.model.context_window = context_window;
        choice.model.max_output = max_output;
        Some(choice.clone())
    }
}

/// Asks every configured provider for its models at once. One switched off is listed
/// without being asked, so the settings can switch it back on.
pub async fn check_providers(config: &Config) -> Result<Vec<ProviderStatus>> {
    let registry = ProviderRegistry::from_config(config)?;
    let all = config.all_providers();
    let mut checks = JoinSet::new();
    for name in registry.names() {
        let name = name.to_owned();
        let Some(provider) = registry.provider(&name) else {
            continue;
        };
        let settings = all.get(&name).cloned();
        checks.spawn(check_provider(name, provider, settings));
    }
    let mut statuses = checks.join_all().await;
    statuses.extend(
        all.iter()
            .filter(|(_, settings)| !settings.enabled)
            .map(|(name, _)| ProviderStatus::disabled(name.clone())),
    );
    statuses.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(statuses)
}

/// Asks one provider for its models and its balance, within the check's time.
pub async fn check_provider(
    name: String,
    provider: Arc<crate::providers::Provider>,
    settings: Option<crate::config::ProviderConfig>,
) -> ProviderStatus {
    let balance = async {
        match &settings {
            Some(settings) => crate::providers::balance::fetch(settings).await,
            None => None,
        }
    };
    let listed = tokio::time::timeout(CHECK_TIMEOUT, provider.list_models());
    let (listed, balance) = tokio::join!(listed, balance);
    let (models, failure) = match listed {
        Ok(Ok(models)) => (Ok(models), None),
        Ok(Err(error)) => {
            let failure = error
                .downcast_ref::<crate::providers::transport::http::CheckFailed>()
                .map(|failed| failed.0);
            (Err(format!("{error:#}")), failure)
        }
        Err(_) => (
            Err("no answer in time".into()),
            Some(dusk_protocol::model::CheckFailure::Unreachable),
        ),
    };
    let hidden = settings
        .map(|settings| settings.hidden_models.into_iter().collect())
        .unwrap_or_default();
    ProviderStatus {
        name,
        models,
        balance,
        limits: provider.limits(),
        enabled: true,
        failure,
        hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The address answers nobody: a provider that is off must not be asked at all.
    const OFF: &str = "providers:\n  off:\n    base_url: 'https://127.0.0.1:1/v1'\n    api_key: k\n    enabled: false\n";

    #[test]
    fn a_saved_model_learns_an_output_cap_without_an_input_window() {
        let config = Config::parse(
            b"model: fixture/model\nproviders:\n  fixture:\n    base_url: https://127.0.0.1:1/v1\n",
        )
        .unwrap();
        let selected = ModelChoice::new(Choice::configured(&config).unwrap().unwrap());
        let status = ProviderStatus {
            name: "fixture".into(),
            models: Ok(vec![Model {
                id: "model".into(),
                context_window: None,
                max_output: Some(262144),
            }]),
            balance: None,
            limits: None,
            enabled: true,
            failure: None,
            hidden: Vec::new(),
        };
        assert!(selected.learn(&[status]));
        assert_eq!(selected.current().unwrap().model.max_output, Some(262144));
        assert_eq!(selected.current().unwrap().model.context_window, None);
    }

    #[tokio::test]
    async fn a_provider_switched_off_is_listed_but_never_asked_or_chosen() {
        let config = Config::parse(OFF.as_bytes()).unwrap();
        let statuses = check_providers(&config).await.unwrap();
        assert_eq!(statuses, [ProviderStatus::disabled("off".into())]);
        assert!(Choice::pick(&config, Some("off/model")).await.is_err());
        let found = Choice::pick(&config, None).await.err().unwrap();
        assert_eq!(found.to_string(), "no provider configured");

        let mut config = Config::parse(format!("{OFF}model: off/model\n").as_bytes()).unwrap();
        assert!(Choice::configured(&config).unwrap().is_none());
        assert!(config.to_yaml().unwrap().contains("enabled: false"));
        assert!(config.set_provider_enabled("off", true));
        assert!(!config.to_yaml().unwrap().contains("enabled"));
        assert!(Choice::configured(&config).unwrap().is_some());
    }
}
