//! The ways a provider's answer fails that the agent tells apart: one that may pass when
//! sent again, an effort or pictures the provider refuses, and an answer cut at the
//! output limit.

use std::{fmt, time::Duration};

/// A failure that may pass if the same request is sent again: the network, a busy or
/// failing server, a cut stream.
#[derive(Debug)]
pub struct Transient(pub String);

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

impl fmt::Display for Transient {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for Transient {}

/// How long the provider asked to wait before the next attempt (`retry-after`).
#[derive(Debug)]
pub struct RetryAfter(pub Duration);

impl fmt::Display for RetryAfter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "retry after {} s", self.0.as_secs())
    }
}

impl std::error::Error for RetryAfter {}

pub fn retry_after(error: &anyhow::Error) -> Option<Duration> {
    error
        .chain()
        .find_map(|cause| cause.downcast_ref::<RetryAfter>().map(|wait| wait.0))
}

/// An error a provider puts in its stream. Without a clear sign that the request itself
/// is wrong (bad input, key, model or size), it is taken as one that may pass.
pub fn stream_error(kind: &str) -> anyhow::Error {
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
    let kind = kind.to_ascii_lowercase();
    if PERMANENT.iter().any(|word| kind.contains(word)) {
        anyhow::anyhow!("provider reported an error during streaming")
    } else {
        Transient("provider reported an error during streaming".into()).into()
    }
}

/// Whether `error` or anything it wraps is a `Transient` failure.
pub fn is_transient(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.downcast_ref::<Transient>().is_some())
}

/// The model wrote as much as it may: the provider cut it at `max_tokens`, or the core
/// stopped the stream at its own count.
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
}
