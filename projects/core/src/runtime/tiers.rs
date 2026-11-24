//! The model a subagent answers on when its asker picks a tier (strong, default or weak)
//! in `ask_agent` or on a task of the team's plan.
//!
//! A tier the settings leave unset, or whose provider is removed or switched off, is the
//! model of the chat that asks: so until the user picks models nothing changes. A team
//! role's own model (`team.models`, `team.model`) stays above the tier. Only subagents
//! read the tiers; new chats, titles and the arbiter keep their own models.

use dusk_protocol::config::{TierState, TierStates};

use crate::config::Config;

pub use dusk_protocol::config::ModelTier as Tier;

/// The model `tier` names, as `provider/model`, when it is set and its provider is
/// connected. `None` keeps the subagent on its chat's model.
pub fn named(tier: Tier, config: &Config) -> Option<&str> {
    let name = config.model_tiers().get(tier)?;
    connected(config, name).then_some(name)
}

/// The model a subagent asked at `tier` answers on, as `provider/model`, when that is
/// not `chat`, the asking chat's model. `None` follows the chat.
pub fn model_for(tier: Option<Tier>, config: &Config, chat: Option<&str>) -> Option<String> {
    let name = named(tier?, config)?;
    (Some(name) != chat).then(|| name.to_owned())
}

/// Every tier as the settings show it.
pub fn states(config: &Config) -> TierStates {
    let state = |tier: Tier| {
        let chosen = config.model_tiers().get(tier).map(str::to_owned);
        let missing = chosen
            .as_deref()
            .is_some_and(|name| !connected(config, name));
        TierState { chosen, missing }
    };
    TierStates {
        strong: state(Tier::Strong),
        default: state(Tier::Default),
        weak: state(Tier::Weak),
    }
}

/// Whether the provider of `name`, `provider/model`, is in the config and switched on.
pub fn connected(config: &Config, name: &str) -> bool {
    name.split_once('/').is_some_and(|(provider, _)| {
        config
            .all_providers()
            .get(provider)
            .is_some_and(|settings| settings.enabled)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "providers:\n  big:\n    base_url: https://127.0.0.1:1/v1\n  small:\n    base_url: https://127.0.0.1:1/v1\nmodel: big/opus\n";

    fn config(extra: &str) -> Config {
        Config::parse(format!("{TWO}{extra}").as_bytes()).unwrap()
    }

    #[test]
    fn an_older_file_loads_with_every_tier_unset_and_writes_none_back() {
        let old = config("");
        assert_eq!(old.model(), Some("big/opus"));
        assert!(old.model_tiers().is_default());
        let yaml = old.to_yaml().unwrap();
        assert!(!yaml.contains("models:"), "{yaml}");
        assert!(yaml.contains("model: big/opus"), "{yaml}");
    }

    #[test]
    fn a_tier_must_name_provider_and_model() {
        for bad in [
            "models:\n  weak: haiku\n",
            "models:\n  strong: '/x'\n",
            "models:\n  fast: big/x\n",
        ] {
            assert!(Config::parse(format!("{TWO}{bad}").as_bytes()).is_err());
        }
    }

    #[test]
    fn an_unset_tier_is_the_chats_model() {
        let config = config("models:\n  strong: big/fable\n");
        for tier in [None, Some(Tier::Default), Some(Tier::Weak)] {
            assert_eq!(model_for(tier, &config, Some("big/opus")), None);
        }
        let states = states(&config);
        assert!(states.weak.chosen.is_none() && !states.weak.missing);
    }

    #[test]
    fn a_set_tier_answers_on_its_model() {
        let config =
            config("models:\n  strong: big/fable\n  default: big/opus\n  weak: small/haiku\n");
        let chat = Some("big/opus");
        assert_eq!(
            model_for(Some(Tier::Strong), &config, chat).as_deref(),
            Some("big/fable")
        );
        assert_eq!(
            model_for(Some(Tier::Weak), &config, chat).as_deref(),
            Some("small/haiku")
        );
        // The same model as the chat's follows the chat.
        assert_eq!(model_for(Some(Tier::Default), &config, chat), None);
        // Without a chat model a set tier still names its own.
        assert_eq!(
            model_for(Some(Tier::Weak), &config, None).as_deref(),
            Some("small/haiku")
        );
    }

    #[test]
    fn a_tier_whose_provider_is_gone_falls_back_and_shows_missing() {
        let mut config = config("models:\n  weak: small/haiku\n");
        assert!(config.set_provider_enabled("small", false));
        assert_eq!(model_for(Some(Tier::Weak), &config, Some("big/opus")), None);
        let shown = states(&config).weak;
        assert_eq!(shown.chosen.as_deref(), Some("small/haiku"));
        assert!(shown.missing);
        assert!(config.remove_provider("small"));
        assert!(states(&config).weak.missing);
        assert_eq!(model_for(Some(Tier::Weak), &config, Some("big/opus")), None);
    }
}
