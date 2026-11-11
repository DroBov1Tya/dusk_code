//! Speech: the servers, recognition and reading aloud.

use crate::runtime::Runtime;
use anyhow::{Context, Result};

/// How many readings are kept to play again.
const SPEECH_CACHE: usize = 8;

/// What a reading is known by: the server, the model, the voice and the text.
fn speech_key(endpoint: &dusk_protocol::config::VoiceEndpoint, text: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [
        endpoint.base_url.as_str(),
        endpoint.model.as_str(),
        endpoint.voice.as_deref().unwrap_or_default(),
        text,
    ] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    hash.finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl Runtime {
    pub fn voice_settings(&self) -> dusk_protocol::config::VoiceSettings {
        self.config().current().voice().clone()
    }

    /// Saves the speech servers after checking their addresses.
    pub fn set_voice(&self, voice: dusk_protocol::config::VoiceSettings) -> Result<()> {
        for endpoint in [&voice.stt, &voice.tts].into_iter().flatten() {
            crate::voice::check_endpoint(endpoint)?;
        }
        self.config().update(|config| config.set_voice(voice))?;
        Ok(())
    }

    /// The key of a speech server, asked of the keychain off the async workers.
    pub(super) async fn voice_key(&self, name: &'static str) -> Result<Option<String>> {
        let Some(store) = self.config().secrets().cloned() else {
            return Ok(None);
        };
        tokio::task::spawn_blocking(move || store.get(name))
            .await
            .context("the keychain call stopped")?
    }

    /// The text of a WAV recording.
    pub async fn transcribe(&self, audio: &[u8]) -> Result<String> {
        let endpoint = self
            .voice_settings()
            .stt
            .context("speech recognition is not set up: see Connectors, Voice")?;
        let key = self.voice_key(dusk_protocol::config::STT_KEY).await?;
        crate::voice::transcribe(&endpoint, key.as_deref(), audio).await
    }

    /// Reads a text aloud; the sound is kept as a card of the artifact store.
    pub async fn speak(&self, text: &str) -> Result<dusk_protocol::tools::Card> {
        let endpoint = self
            .voice_settings()
            .tts
            .context("speech synthesis is not set up: see Connectors, Voice")?;
        let key = self.voice_key(dusk_protocol::config::TTS_KEY).await?;
        let asked = speech_key(&endpoint, text);
        if let Some((_, card)) = self.speech.lock().iter().find(|(known, _)| *known == asked) {
            return Ok(card.clone());
        }
        let sound = crate::voice::speak(&endpoint, key.as_deref(), text).await?;
        let store =
            crate::state::artifacts::ArtifactStore::new(self.launcher.paths.artifacts_dir());
        let card = crate::tools::save_sound(&store, &sound, Some("Read aloud".to_owned()))?;
        let mut speech = self.speech.lock();
        speech.push_back((asked, card.clone()));
        while speech.len() > SPEECH_CACHE {
            if let Some((_, old)) = speech.pop_front() {
                store.remove(&old);
            }
        }
        Ok(card)
    }

    /// The speech models of this machine and where each is.
    pub async fn voice_models(&self) -> dusk_protocol::config::VoiceModels {
        self.local_voice.list(&self.voice_settings()).await
    }

    /// Installs a model, starts it and sets the speech to it; the call returns at once.
    pub async fn install_voice_model(&self, id: &str) -> Result<()> {
        self.local_voice.install(id).await
    }

    pub async fn remove_voice_model(&self, id: &str) -> Result<()> {
        self.local_voice.remove(id).await
    }
}
