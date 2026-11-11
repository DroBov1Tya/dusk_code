//! The secrets a user keeps by hand.

use super::providers::checked_key;
use crate::runtime::Runtime;
use anyhow::{Context, Result, ensure};

/// The secrets a user sets by hand: the Hugging Face token and the keys of the speech servers.
fn is_user_secret(name: &str) -> bool {
    use dusk_protocol::config::{STT_KEY, TTS_KEY};
    [crate::secrets::HUGGINGFACE, STT_KEY, TTS_KEY].contains(&name)
}

impl Runtime {
    /// Which secrets are set, never their values. The keychain is asked off the async
    /// workers, as it may wait for the user to unlock it.
    pub async fn secrets_info(&self) -> dusk_protocol::config::SecretsInfo {
        use dusk_protocol::config::{HUGGINGFACE, SecretInfo, SecretsInfo};
        let config = self.config();
        let store = config.secrets().cloned();
        let mut problem = None;
        let mut token = false;
        if let Some(store) = store.clone() {
            match tokio::task::spawn_blocking(move || store.get(HUGGINGFACE)).await {
                Ok(Ok(found)) => token = found.is_some(),
                Ok(Err(error)) => problem = Some(format!("{error:#}")),
                Err(error) => problem = Some(error.to_string()),
            }
        }
        let mut entries = vec![SecretInfo {
            name: HUGGINGFACE.to_owned(),
            label: "Hugging Face token".to_owned(),
            set: token,
            editable: store.is_some(),
        }];
        for (name, label) in [
            (
                dusk_protocol::config::STT_KEY,
                "Key of the speech recognition server",
            ),
            (
                dusk_protocol::config::TTS_KEY,
                "Key of the speech synthesis server",
            ),
        ] {
            let set = match store.clone() {
                Some(store) => {
                    let asked = name.to_owned();
                    tokio::task::spawn_blocking(move || store.get(&asked))
                        .await
                        .is_ok_and(|found| found.ok().flatten().is_some())
                }
                None => false,
            };
            entries.push(SecretInfo {
                name: name.to_owned(),
                label: label.to_owned(),
                set,
                editable: store.is_some(),
            });
        }
        for (name, provider) in config.current().providers() {
            entries.push(SecretInfo {
                name: crate::secrets::provider_key(name),
                label: format!("API key of {name}"),
                set: !provider.api_key.0.is_empty(),
                editable: false,
            });
        }
        SecretsInfo {
            keychain: store.is_some(),
            problem,
            entries,
        }
    }

    /// Keeps the Hugging Face token. Provider keys go with their providers; no other name
    /// is taken.
    pub async fn set_secret(&self, name: &str, value: &str) -> Result<()> {
        ensure!(is_user_secret(name), "{name} is not a secret to set here");
        let value = checked_key(value.to_owned())?;
        ensure!(!value.is_empty(), "a token is not empty");
        let store = self
            .config()
            .secrets()
            .context("this machine has no system keychain to keep it in")?
            .clone();
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || store.set(&name, &value))
            .await
            .context("the keychain call stopped")?
    }

    pub async fn delete_secret(&self, name: &str) -> Result<()> {
        ensure!(is_user_secret(name), "{name} is not a secret to clear here");
        let Some(store) = self.config().secrets().cloned() else {
            return Ok(());
        };
        let name = name.to_owned();
        tokio::task::spawn_blocking(move || store.delete(&name))
            .await
            .context("the keychain call stopped")?
    }
}
