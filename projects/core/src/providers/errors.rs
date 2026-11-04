//! The ways a provider's answer fails that the agent tells apart: one that may pass when
//! sent again, an effort or pictures the provider refuses, and an answer cut at the
//! output limit.

use std::fmt;

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
