//! The Workshop for the model: `search_workshop` finds listings in the store the core
//! reads for every client, and `suggest_from_workshop` shows some of them to the user as a
//! card in the chat. Neither installs, enables or changes anything: the card's Add opens
//! the Workshop's own install, with its confirmation, and only the user's click does.
//! The card's rows are the store's listings, found by the ids the model gave; nothing
//! the model wrote but the heading reaches the card.

mod search;
mod suggest;
#[cfg(test)]
mod tests;

pub use search::SearchWorkshop;
pub use suggest::SuggestFromWorkshop;

use anyhow::{Result, bail};
use dusk_protocol::plugin::{Order, StoreList};

use crate::{config::Config, init::paths::Paths, plugins::store};

/// The store as the Workshop lists it: every source, every kind, in trust order. A list
/// with no listing and a source that failed is an error, so the model can say the store
/// could not be read instead of that it offers nothing.
async fn read(paths: &Paths, config: &Config) -> Result<StoreList> {
    let on = config
        .plugins()
        .iter()
        .filter(|(_, plugin)| plugin.enabled)
        .map(|(name, _)| name.clone())
        .collect();
    let here = store::Here::of(config, paths, on);
    let mirrors = &config.workshop().sources;
    let list = store::list(paths, mirrors, Order::Trust, None, &[], &here).await?;
    if list.listings.is_empty() && !list.failed.is_empty() {
        let failed: Vec<String> = list
            .failed
            .iter()
            .map(|failure| format!("{}: {}", failure.source, failure.error))
            .collect();
        bail!("the Workshop could not be read ({})", failed.join("; "));
    }
    Ok(list)
}
