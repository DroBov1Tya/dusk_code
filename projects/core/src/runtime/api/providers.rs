//! Connecting and forgetting providers.

use crate::runtime::Runtime;
use crate::{
    config::{ApiKey, ProviderConfig, ProviderKind},
    providers::Provider,
};
use anyhow::{Context, Result, ensure};
use dusk_protocol::{client::ProviderTarget, model::ProviderStatus};

use super::super::model_choice::check_provider;
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

/// A provider as typed in a form, checked without a network request.
fn new_provider(
    kind: ProviderKind,
    base_url: String,
    api_key: Option<String>,
) -> Result<ProviderConfig> {
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
        enabled: true,
    };
    crate::providers::validate_provider(&provider)?;
    Ok(provider)
}

impl Runtime {
    /// Asks a provider whether it answers: one typed in a form and not saved, or a saved one.
    /// Nothing is saved or changed.
    pub async fn check_provider(&self, target: ProviderTarget) -> Result<ProviderStatus> {
        let (name, settings) = match target {
            ProviderTarget::New {
                kind,
                base_url,
                api_key,
            } => (
                String::new(),
                new_provider(kind, base_url, api_key.map(|key| key.0))?,
            ),
            ProviderTarget::Saved { name } => {
                let settings = self
                    .launcher
                    .config
                    .current()
                    .all_providers()
                    .get(&name)
                    .cloned()
                    .with_context(|| format!("no provider named {name}"))?;
                (name, settings)
            }
        };
        if !settings.enabled {
            return Ok(ProviderStatus::disabled(name));
        }
        let provider = Arc::new(Provider::new(&settings)?);
        Ok(check_provider(name, provider, Some(settings)).await)
    }

    /// Builds the connection of a saved provider anew, with a new key when one is given. The
    /// answer of the provider is for `check_provider` to find out.
    pub fn reconnect_provider(&self, name: &str, api_key: Option<String>) -> Result<()> {
        let saved = self
            .launcher
            .config
            .current()
            .providers()
            .get(name)
            .cloned()
            .with_context(|| format!("no provider of the config file is named {name}"))?;
        let provider = match api_key {
            Some(key) => {
                let key = checked_key(key)?;
                ensure!(!key.is_empty(), "a new key is not empty");
                let mut changed = saved;
                changed.api_key = ApiKey(key);
                crate::providers::validate_provider(&changed)?;
                let kept = changed.clone();
                self.launcher
                    .config
                    .update(|config| config.set_provider(name.to_owned(), kept))?;
                changed
            }
            None => saved,
        };
        let adapter = Arc::new(Provider::new(&provider)?);
        self.launcher.model.replace_provider(name, adapter);
        Ok(())
    }

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
        let provider = new_provider(kind, base_url, api_key)?;
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
        self.ensure_not_chosen(name)?;
        self.launcher.config.update(|config| {
            config.remove_provider(name);
        })?;
        Ok(())
    }

    /// Switches a provider of the config file on or off; its key stays. Off is refused
    /// like a removal while it holds the chosen model.
    pub fn set_provider_enabled(&self, name: &str, enabled: bool) -> Result<()> {
        if !enabled {
            self.ensure_not_chosen(name)?;
            let chosen = self.launcher.model.current();
            // Discovery may have chosen a model the config does not name yet.
            ensure!(
                chosen.is_none_or(|choice| choice.provider_name != name),
                "{name} holds the chosen model; choose a model of another provider first"
            );
        }
        let mut found = false;
        self.launcher
            .config
            .update(|config| found = config.set_provider_enabled(name, enabled))?;
        ensure!(found, "no provider named {name}");
        Ok(())
    }

    /// A provider of the config file that does not hold the chosen model: the chat would
    /// lose its model under the user.
    fn ensure_not_chosen(&self, name: &str) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    /// A server on this machine that answers any request with `status` and `body`.
    async fn serve(status: &'static str, body: &'static str) -> String {
        serve_with(status, "", body).await
    }

    /// The same, with header lines of its own, each ended by `\r\n`.
    async fn serve_with(status: &'static str, headers: &'static str, body: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buffer = [0u8; 4096];
                let _ = socket.read(&mut buffer).await;
                let reply = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        base
    }

    async fn check(base_url: String, key: &str) -> ProviderStatus {
        let settings = new_provider(
            ProviderKind::OpenAiCompatible,
            base_url,
            Some(key.to_owned()),
        )
        .unwrap();
        let provider = Arc::new(Provider::new(&settings).unwrap());
        check_provider(String::new(), provider, Some(settings)).await
    }

    #[tokio::test]
    async fn a_server_that_lists_its_models_answers() {
        let base = serve("200 OK", r#"{"data":[{"id":"a"},{"id":"b"}]}"#).await;
        let status = check(base, "sk-test").await;
        assert_eq!(status.models.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn limits_an_answer_reported_are_in_the_status_and_kept_for_the_account() {
        let headers = "x-ratelimit-limit-requests: 5000\r\n\
                       x-ratelimit-remaining-requests: 4812\r\n\
                       x-ratelimit-reset-requests: 12s\r\n";
        let base = serve_with("200 OK", headers, r#"{"data":[{"id":"a"}]}"#).await;
        let requests = check(base.clone(), "sk-test")
            .await
            .limits
            .and_then(|limits| limits.requests)
            .expect("the limits of the answer");
        assert_eq!((requests.limit, requests.remaining), (5_000, 4_812));
        // A provider built anew for the same account knows them before it asks anything.
        let settings =
            new_provider(ProviderKind::OpenAiCompatible, base, Some("sk-test".into())).unwrap();
        assert!(Provider::new(&settings).unwrap().limits().is_some());

        let silent = serve("200 OK", r#"{"data":[{"id":"a"}]}"#).await;
        assert_eq!(check(silent, "sk-test").await.limits, None);
    }

    #[tokio::test]
    async fn a_server_that_refuses_the_key_is_told_apart_from_one_that_answers() {
        let base = serve("401 Unauthorized", r#"{"error":"bad key"}"#).await;
        let error = check(base, "sk-wrong").await.models.unwrap_err();
        assert!(
            error.contains("401") || error.contains("rejected"),
            "{error}"
        );
        assert!(!error.contains("sk-wrong"), "{error}");
    }

    #[tokio::test]
    async fn a_new_provider_is_checked_by_its_address_before_any_request() {
        assert!(
            new_provider(
                ProviderKind::OpenAiCompatible,
                "http://example.com/v1".into(),
                None
            )
            .is_err()
        );
        assert!(
            new_provider(
                ProviderKind::OpenAiCompatible,
                "https://api.example.com/v1".into(),
                Some("a\nb".into())
            )
            .is_err()
        );
    }
}
