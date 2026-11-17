//! Why a call to a provider failed.

use serde::{Deserialize, Serialize};

/// What went wrong with a provider call, coarse enough for a client to draw and for the
/// retry loop to decide on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderErrorKind {
    /// No response at all: the name did not resolve, the connection or TLS failed, or it
    /// was reset before the provider answered.
    Unreachable,
    /// Nothing arrived within the idle or model timeout.
    TimedOut,
    /// The provider asked to slow down (HTTP 429).
    RateLimited,
    /// The provider is busy or failing (HTTP 5xx, 529, an "overloaded" stream error).
    Overloaded,
    /// The key or sign-in was not accepted (HTTP 401, 403).
    Auth,
    /// The account or plan has nothing left to spend (HTTP 402, a spent plan).
    Quota,
    /// The provider refused the request itself: bad input, no such model, unsupported.
    Refused,
    /// The provider said the context is too large.
    TooLong,
    /// The answer broke off after it began.
    Interrupted,
}

impl ProviderErrorKind {
    /// Whether the same request may pass when sent again.
    pub fn is_retryable(self) -> bool {
        match self {
            Self::Unreachable
            | Self::TimedOut
            | Self::RateLimited
            | Self::Overloaded
            | Self::Interrupted => true,
            Self::Auth | Self::Quota | Self::Refused | Self::TooLong => false,
        }
    }

    /// What every client calls it, in a few words.
    pub fn label(self) -> &'static str {
        match self {
            Self::Unreachable => "Provider unreachable",
            Self::TimedOut => "Provider timed out",
            Self::RateLimited => "Rate limited",
            Self::Overloaded => "Provider overloaded",
            Self::Auth => "Key or sign-in not accepted",
            Self::Quota => "Quota used up",
            Self::Refused => "Request refused",
            Self::TooLong => "Context too long",
            Self::Interrupted => "Answer interrupted",
        }
    }

    /// Whether it says the provider cannot serve now, not that this request was wrong.
    pub fn is_providers_fault(self) -> bool {
        !matches!(self, Self::Refused | Self::TooLong)
    }
}

/// A model call that failed for good: why, and how many times it was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFailure {
    pub kind: ProviderErrorKind,
    /// The first attempt included.
    pub attempts: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_failures_outside_the_request_are_retried() {
        use ProviderErrorKind::*;
        for kind in [Unreachable, TimedOut, RateLimited, Overloaded, Interrupted] {
            assert!(kind.is_retryable(), "{kind:?}");
        }
        for kind in [Auth, Quota, Refused, TooLong] {
            assert!(!kind.is_retryable(), "{kind:?}");
        }
    }
}
