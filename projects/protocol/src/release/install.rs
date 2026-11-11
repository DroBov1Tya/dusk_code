//! How far an install got.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstallStage {
    Preparing,
    Downloading,
    Verifying,
    Installing,
    Complete,
}

/// Download byte counts stay available through verification and installation.
/// A missing or zero content length does not give a useful percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallProgress {
    pub stage: InstallStage,
    pub downloaded: u64,
    pub total: Option<u64>,
}

impl Default for InstallProgress {
    fn default() -> Self {
        Self {
            stage: InstallStage::Preparing,
            downloaded: 0,
            total: None,
        }
    }
}
