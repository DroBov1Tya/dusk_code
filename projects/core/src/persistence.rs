//! Async snapshot boundary. Storage backends and serialization are separate work.

use crate::{api::ApiVersion, lifecycle::LifecycleState};
use anyhow::Result;
use std::future::Future;

#[derive(Debug, Clone)]
pub struct CoreSnapshot {
    pub api_version: ApiVersion,
    pub state: LifecycleState,
}

pub trait Persistence: Send + Sync {
    fn load(&self) -> impl Future<Output = Result<Option<CoreSnapshot>>> + Send;

    fn save(&self, snapshot: &CoreSnapshot) -> impl Future<Output = Result<()>> + Send;
}
