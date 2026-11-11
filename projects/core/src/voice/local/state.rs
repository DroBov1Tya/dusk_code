//! Which models the chat's speech was pointed at, so they start again with the app.

use std::collections::BTreeSet;

use anyhow::{Context, Result};

use super::{LocalVoice, catalog::Model};

const FILE: &str = "chosen.json";

impl LocalVoice {
    /// The ids chosen: at most one for each way a model works.
    pub(super) fn chosen(&self) -> BTreeSet<String> {
        std::fs::read(self.root.join(FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Chooses a model, and drops the other one of its kind.
    pub(super) fn choose(&self, model: &Model) -> Result<()> {
        let mut chosen = self.chosen();
        chosen.retain(|id| super::find(id).is_some_and(|other| other.kind != model.kind));
        chosen.insert(model.id.to_owned());
        self.save(&chosen)
    }

    pub(super) fn forget(&self, model: &Model) -> Result<()> {
        let mut chosen = self.chosen();
        if chosen.remove(model.id) {
            self.save(&chosen)?;
        }
        Ok(())
    }

    fn save(&self, chosen: &BTreeSet<String>) -> Result<()> {
        std::fs::create_dir_all(&self.root).context("cannot make the voice directory")?;
        std::fs::write(self.root.join(FILE), serde_json::to_vec(chosen)?)
            .context("cannot save the chosen speech models")
    }
}
