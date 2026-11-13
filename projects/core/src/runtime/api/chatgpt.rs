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

/// The ChatGPT provider of the config file, and its record when it can be read: with a
/// locked keychain or a damaged record there is a provider and no record.
fn signed_in(config: &LiveConfig) -> Option<(String, Option<Credential>)> {
    config
        .current()
        .providers()
        .iter()
        .find(|(_, provider)| provider.kind == ProviderKind::ChatGpt)
        .map(|(name, provider)| (name.clone(), Credential::read(&provider.api_key.0).ok()))
}

/// Keeps a renewed record, and reads the stored one before a renewal: the refresh token
/// rotates, so a spent one in the keychain would end the sign-in at the next start, and
/// another client of this program may have renewed it since.
pub(crate) fn keep_renewed(config: Arc<LiveConfig>) {
    let stored = Arc::clone(&config);
    sign_in::keep_in(sign_in::Store {
        load: Box::new(move |client_id| {
            // The file and the keychain as they are now, not as this process last saw them.
            let _ = stored.reload();
            let (_, credential) = signed_in(&stored)?;
            credential.filter(|credential| credential.client_id == client_id)
        }),
        save: Box::new(move |credential| {
            let Some((name, _)) = signed_in(&config) else {
                // Removed meanwhile: nothing to keep.
                return Ok(());
            };
            let renewed = provider(credential);
            config.update(|config| config.set_provider(name, renewed))?;
            Ok(())
        }),
    });
}

impl Runtime {
    /// How the sign-in stands, for the status every client draws from.
    pub fn chatgpt(&self) -> ChatGpt {
        let note = self.chatgpt.lock().clone();
        match (note, signed_in(&self.launcher.config)) {
            (ChatGpt::Waiting, _) => ChatGpt::Waiting,
            (_, Some((_, Some(credential)))) => ChatGpt::SignedIn {
                account: credential.email.unwrap_or_else(|| "ChatGPT".to_owned()),
            },
            (failed @ ChatGpt::Failed(_), _) => failed,
            (_, Some((_, None))) => {
                ChatGpt::Failed("the saved ChatGPT sign-in cannot be read; sign in again".into())
            }
            _ => ChatGpt::SignedOut,
        }
    }

    /// Starts a sign-in with ChatGPT: returns the page for the user's browser, and saves
    /// the provider once they come back from it. Signed in before, the same account
    /// signs in again.
    pub async fn sign_in_chatgpt(&self) -> Result<reqwest::Url> {
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
        // One at a time: checked and marked in one step, before anything waits.
        {
            let mut note = self.chatgpt.lock();
            ensure!(
                *note != ChatGpt::Waiting,
                "a sign-in is already waiting for the browser"
            );
            *note = ChatGpt::Waiting;
        }
        // A record that cannot be read signs in as the first time, under the same name.
        let started = sign_in::sign_in(earlier.and_then(|(_, credential)| credential)).await;
        let (page, finish) = match started {
            Ok(started) => started,
            Err(error) => {
                *self.chatgpt.lock() = ChatGpt::SignedOut;
                return Err(error);
            }
        };
        let (config, model, note) = (
            Arc::clone(&self.launcher.config),
            Arc::clone(&self.launcher.model),
            Arc::clone(&self.chatgpt),
        );
        tokio::spawn(async move {
            let saved = async {
                let settings = provider(&finish.await?);
                // Saved first: the session reads the stored record before it renews.
                config.update(|config| config.set_provider(name.clone(), settings.clone()))?;
                model.replace_provider(&name, Arc::new(Provider::new(&settings)?));
                anyhow::Ok(())
            };
            *note.lock() = match saved.await {
                Ok(()) => {
                    info!("signed in to ChatGPT");
                    ChatGpt::SignedOut
                }
                Err(error) => {
                    // Our own words and filtered codes: no server's prose reaches here.
                    let error: String = format!("{error:#}").chars().take(200).collect();
                    warn!("sign-in to ChatGPT failed: {error}");
                    ChatGpt::Failed(error)
                }
            };
        });
        Ok(page)
    }
}
