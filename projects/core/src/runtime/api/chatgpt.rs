//! Signing in with a ChatGPT account, and keeping what the sign-in gave.

use std::sync::Arc;

use anyhow::{Result, ensure};
use dusk_protocol::client::ChatGpt;
use tracing::{info, warn};

use crate::{
    config::{ApiKey, ProviderConfig, ProviderKind, live::LiveConfig},
    providers::{
        Provider,
        openai::sign_in::{self, Credential, RESOURCE},
    },
    runtime::Runtime,
};

/// The name the provider gets in the config and in `provider/model`.
const NAME: &str = "chatgpt";

/// The provider a sign-in is kept as: the record stands where a key would.
fn provider(credential: &Credential) -> ProviderConfig {
    ProviderConfig {
        kind: ProviderKind::ChatGpt,
        base_url: RESOURCE.to_owned(),
        api_key: ApiKey(credential.record()),
        inline_tags: false,
        allow_http: false,
        prices: Default::default(),
        balance: None,
        requests_per_minute: None,
        enabled: true,
    }
}

/// The signed-in provider of the config file and its record, when there is one.
fn signed_in(config: &LiveConfig) -> Option<(String, Credential)> {
    config
        .current()
        .providers()
        .iter()
        .filter(|(_, provider)| provider.kind == ProviderKind::ChatGpt)
        .find_map(|(name, provider)| {
            Some((name.clone(), Credential::read(&provider.api_key.0).ok()?))
        })
}

/// Keeps a renewed record: the refresh token rotates, so the one in the keychain would
/// be of no use after the next start.
pub(crate) fn keep_renewed(config: Arc<LiveConfig>) {
    sign_in::save_with(move |credential| {
        let Some((name, earlier)) = signed_in(&config) else {
            return;
        };
        if earlier.client_id != credential.client_id {
            return;
        }
        let renewed = provider(credential);
        if let Err(error) = config.update(|config| config.set_provider(name, renewed)) {
            warn!("the renewed ChatGPT sign-in was not saved: {error:#}");
        }
    });
}

impl Runtime {
    /// How the sign-in stands, for the status every client draws from.
    pub fn chatgpt(&self) -> ChatGpt {
        let note = self.chatgpt.lock().clone();
        match (note, signed_in(&self.launcher.config)) {
            (ChatGpt::Waiting, _) => ChatGpt::Waiting,
            (_, Some((_, credential))) => ChatGpt::SignedIn {
                account: credential.email.unwrap_or_else(|| "ChatGPT".to_owned()),
            },
            (failed @ ChatGpt::Failed(_), None) => failed,
            _ => ChatGpt::SignedOut,
        }
    }

    /// Starts a sign-in with ChatGPT: returns the page for the user's browser, and saves
    /// the provider once they come back from it. Signed in before, the same account
    /// signs in again.
    pub async fn sign_in_chatgpt(&self) -> Result<reqwest::Url> {
        ensure!(
            *self.chatgpt.lock() != ChatGpt::Waiting,
            "a sign-in is already waiting for the browser"
        );
        let earlier = signed_in(&self.launcher.config);
        let name = earlier.as_ref().map_or(NAME, |(name, _)| name).to_owned();
        ensure!(
            earlier.is_some()
                || !self
                    .launcher
                    .config
                    .current()
                    .providers()
                    .contains_key(NAME),
            "a provider named {NAME} exists; remove or rename it first"
        );
        let (page, finish) = sign_in::sign_in(earlier.map(|(_, credential)| credential)).await?;
        *self.chatgpt.lock() = ChatGpt::Waiting;
        let (config, model, note) = (
            Arc::clone(&self.launcher.config),
            Arc::clone(&self.launcher.model),
            Arc::clone(&self.chatgpt),
        );
        tokio::spawn(async move {
            let saved = async {
                let settings = provider(&finish.await?);
                let adapter = Arc::new(Provider::new(&settings)?);
                config.update(|config| config.set_provider(name.clone(), settings))?;
                model.replace_provider(&name, adapter);
                anyhow::Ok(())
            };
            *note.lock() = match saved.await {
                Ok(()) => {
                    info!("signed in to ChatGPT");
                    ChatGpt::SignedOut
                }
                Err(error) => {
                    // Our own words only: the servers' answers never reach an error here.
                    let error: String = format!("{error:#}").chars().take(200).collect();
                    warn!("sign-in to ChatGPT failed: {error}");
                    ChatGpt::Failed(error)
                }
            };
        });
        Ok(page)
    }
}
