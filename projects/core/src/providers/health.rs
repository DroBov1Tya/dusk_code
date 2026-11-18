//! How each provider's calls go, learned from the calls turns make anyway: never stored,
//! never asked for on its own. A provider keeps one state until it turns healthy or
//! failing or fails for another reason, so the status changes only then.

use std::collections::BTreeMap;

use dusk_protocol::model::{CallHealth, ProviderErrorKind, ProviderHealth};
use parking_lot::Mutex;

#[derive(Default)]
pub struct Health {
    providers: Mutex<BTreeMap<String, Calls>>,
}

/// What is shown of a provider, and when its newest call was answered: that time is shown
/// only once the provider fails, since it changes with every answered call.
#[derive(Clone, Copy)]
struct Calls {
    shown: CallHealth,
    last_answered: Option<i64>,
}

impl Health {
    /// A call to `provider` was answered at `now`. True when that changed its state.
    pub fn answered(&self, provider: &str, now: i64) -> bool {
        self.update(provider, |old| {
            let shown = match old.map(|calls| calls.shown) {
                Some(answering @ CallHealth::Answering { .. }) => answering,
                _ => CallHealth::Answering { since: now },
            };
            Calls {
                shown,
                last_answered: Some(now),
            }
        })
    }

    /// A call to `provider` failed with `kind` at `now`. True when that changed its state.
    pub fn failed(&self, provider: &str, kind: ProviderErrorKind, now: i64) -> bool {
        self.update(provider, |old| {
            let last_answered = old.and_then(|calls| calls.last_answered);
            let since = match old.map(|calls| calls.shown) {
                Some(CallHealth::Failing { since, .. }) => since,
                _ => now,
            };
            Calls {
                shown: CallHealth::Failing {
                    kind,
                    since,
                    last_answered,
                },
                last_answered,
            }
        })
    }

    /// The providers `configured` names, by name.
    pub fn of(&self, configured: impl Fn(&str) -> bool) -> Vec<ProviderHealth> {
        self.providers
            .lock()
            .iter()
            .filter(|(name, _)| configured(name))
            .map(|(name, calls)| ProviderHealth {
                name: name.clone(),
                calls: calls.shown,
            })
            .collect()
    }

    fn update(&self, provider: &str, next: impl FnOnce(Option<Calls>) -> Calls) -> bool {
        let mut providers = self.providers.lock();
        let old = providers.get(provider).copied();
        let new = next(old);
        providers.insert(provider.to_owned(), new);
        old.map(|calls| calls.shown) != Some(new.shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ProviderErrorKind::{TimedOut, Unreachable};

    /// Only a change of class or of the failure's kind is a change; the calls in between
    /// leave the state as it was.
    #[test]
    fn a_provider_changes_state_only_when_its_calls_turn() {
        let health = Health::default();
        let steps = [
            (Some(Unreachable), 10, true),
            (Some(Unreachable), 20, false),
            (Some(TimedOut), 30, true),
            (None, 40, true),
            (None, 50, false),
            (Some(Unreachable), 60, true),
        ];
        for (failure, now, changed) in steps {
            let did = match failure {
                Some(kind) => health.failed("main", kind, now),
                None => health.answered("main", now),
            };
            assert_eq!(did, changed, "at {now}");
        }
        assert_eq!(
            health.of(|name| name == "main"),
            [ProviderHealth {
                name: "main".into(),
                calls: CallHealth::Failing {
                    kind: Unreachable,
                    since: 60,
                    last_answered: Some(50),
                },
            }]
        );
        assert!(health.of(|_| false).is_empty());
    }
}
