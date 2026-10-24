//! Async authorization contract; runtime enforcement must call this boundary.

use std::future::Future;

use anyhow::Result;

#[derive(Debug, Clone, Copy)]
pub struct PolicyRequest<'a> {
    pub extension_id: &'a str,
    pub operation: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    Allow,
    Deny { reason: String },
}

pub trait Policy: Send + Sync {
    fn authorize(
        &self,
        request: PolicyRequest<'_>,
    ) -> impl Future<Output = Result<PolicyDecision>> + Send;
}
