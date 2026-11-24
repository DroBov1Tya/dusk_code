//! The ways a provider's answer fails that the agent tells apart: a classified failure
//! the retry loop decides on, an effort or pictures the provider refuses, and an answer
//! cut at the output limit.

use std::{fmt, time::Duration};

pub use dusk_protocol::model::ProviderErrorKind;

/// A provider call that failed, with what the retry loop, the log and the user need.
#[derive(Debug, Clone)]
pub struct ProviderError {
    pub kind: ProviderErrorKind,
    /// Safe to show: never a key, a header, the request body, or a URL path or query.
    pub message: String,
    pub status: Option<u16>,
    /// How long the provider asked to wait (`retry-after`), capped.
    pub retry_after: Option<Duration>,
    /// What lies under it, for the log only; held to the same rule as `message`.
    pub cause: Option<String>,
}

impl ProviderError {
    pub fn new(kind: ProviderErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            status: None,
            retry_after: None,
            cause: None,
        }
    }
}

impl fmt::Display for ProviderError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ProviderError {}

/// The provider failure in `error` or anything it wraps.
pub fn provider_error(error: &anyhow::Error) -> Option<&ProviderError> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<ProviderError>())
}

/// The provider explicitly rejected `reasoning_effort: none`. This is permanent for
/// this request and must never trigger a resend with a different effort.
#[derive(Debug)]
pub struct UnsupportedEffort;

impl fmt::Display for UnsupportedEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("provider does not support reasoning effort off; request was not resent")
    }
}

impl std::error::Error for UnsupportedEffort {}

pub fn is_unsupported_effort(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<UnsupportedEffort>().is_some())
}

/// The provider explicitly rejected the level of reasoning effort a request carried
/// (a level other than off), as a model without reasoning does. It wraps the provider's
/// own failure, so a caller that does not drop the effort sees that failure unchanged.
#[derive(Debug)]
pub struct RefusedEffort(pub anyhow::Error);

impl fmt::Display for RefusedEffort {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, formatter)
    }
}

impl std::error::Error for RefusedEffort {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.0.as_ref())
    }
}

pub fn is_refused_effort(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<RefusedEffort>().is_some())
}

/// The provider refused a request that carried pictures. It may say nothing about why,
/// so the same request without them tells.
#[derive(Debug)]
pub struct UnsupportedPictures;

impl fmt::Display for UnsupportedPictures {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("provider refused a request with pictures")
    }
}

impl std::error::Error for UnsupportedPictures {}

pub fn is_unsupported_pictures(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<UnsupportedPictures>().is_some())
}

/// A stream that ended before the answer was complete.
pub fn ended_early() -> ProviderError {
    ProviderError::new(
        ProviderErrorKind::Interrupted,
        "provider stream ended before the response was complete",
    )
}

/// An error a provider puts in its stream, from its type or code alone. Without a clear
/// sign that the request itself is wrong (bad input, key, model or size), it is taken as
/// one that may pass.
pub fn stream_error(kind: &str) -> anyhow::Error {
    ProviderError::new(
        stream_error_kind(kind),
        "provider reported an error during streaming",
    )
    .into()
}

fn stream_error_kind(kind: &str) -> ProviderErrorKind {
    use ProviderErrorKind::*;
    let kind = kind.to_ascii_lowercase();
    let says = |words: &[&str]| words.iter().any(|word| kind.contains(word));
    // The request's own faults first, as they never pass on a resend.
    if says(&["too_large", "context_length"]) {
        TooLong
    } else if says(&["authentication", "permission", "401", "403"]) {
        Auth
    } else if says(&["invalid", "not_found", "content_filter", "404"]) {
        Refused
    } else if says(&["rate_limit", "resource_exhausted", "429"]) {
        RateLimited
    } else if says(&[
        "overloaded",
        "unavailable",
        "server_error",
        "api_error",
        "500",
        "502",
        "503",
        "504",
        "529",
    ]) {
        Overloaded
    } else {
        Interrupted
    }
}

/// Whether `error` or anything it wraps is a failure that may pass when sent again.
#[cfg(test)]
pub fn is_transient(error: &anyhow::Error) -> bool {
    provider_error(error).is_some_and(|failure| failure.kind.is_retryable())
}

/// The provider stopped the response at its output token budget.
#[derive(Debug)]
pub struct OutputLimit;

impl fmt::Display for OutputLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the answer reached the output token limit")
    }
}

impl std::error::Error for OutputLimit {}

pub fn is_output_limit(error: &anyhow::Error) -> bool {
    error.downcast_ref::<OutputLimit>().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_errors_without_a_clear_cause_may_pass() {
        assert!(is_transient(&stream_error("")));
        assert!(is_transient(&stream_error("server_error 500")));
        assert!(!is_transient(&stream_error("invalid_request_error")));
        assert!(!is_transient(&stream_error("context_length_exceeded")));
    }

    /// The words the old classification refused to resend; every other stream error was
    /// sent again.
    const PERMANENT: [&str; 10] = [
        "invalid",
        "authentication",
        "permission",
        "not_found",
        "too_large",
        "context_length",
        "content_filter",
        "401",
        "403",
        "404",
    ];

    #[test]
    fn stream_errors_are_classified_and_retried_as_before() {
        use ProviderErrorKind::*;
        let table = [
            ("", Interrupted),
            ("overloaded_error", Overloaded),
            ("api_error", Overloaded),
            ("\"server_error\" null", Overloaded),
            ("UNAVAILABLE 503", Overloaded),
            ("INTERNAL 500", Overloaded),
            ("rate_limit_error", RateLimited),
            ("RESOURCE_EXHAUSTED 429", RateLimited),
            ("authentication_error", Auth),
            ("permission_error", Auth),
            ("UNAUTHENTICATED 401", Auth),
            ("PERMISSION_DENIED 403", Auth),
            ("invalid_request_error", Refused),
            ("not_found_error", Refused),
            ("NOT_FOUND 404", Refused),
            ("content_filter", Refused),
            ("request_too_large", TooLong),
            ("invalid_request_error context_length_exceeded", TooLong),
            ("something new", Interrupted),
        ];
        for (code, expected) in table {
            let error = stream_error(code);
            assert_eq!(
                provider_error(&error).map(|e| e.kind),
                Some(expected),
                "{code}"
            );
            let lower = code.to_ascii_lowercase();
            let was_transient = !PERMANENT.iter().any(|word| lower.contains(word));
            assert_eq!(is_transient(&error), was_transient, "{code}");
        }
    }
}
